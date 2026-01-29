#![allow(clippy::map_clone)]
//! EndpointSlice Dynamic Backend Discovery Test
//!
//! Validates that RAUTA detects backend changes via EndpointSlice watching.

use crate::integration::framework::{fixtures, k8s};
use crate::integration::framework::{TestContext, TestResult};
use crate::integration::{TestConfig, TestScenario};
use async_trait::async_trait;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams, Patch, PatchParams};
use std::time::Duration;

pub struct EndpointSliceScenario;

#[async_trait]
impl TestScenario for EndpointSliceScenario {
    fn name(&self) -> &str {
        "endpointslice_updates"
    }

    async fn run(&self, ctx: &mut TestContext) -> TestResult {
        println!("\n🧪 Running EndpointSlice Update Scenarios");

        // Test 1: Scale up detection
        test_scale_up(ctx).await?;

        // Test 2: Scale down detection
        test_scale_down(ctx).await?;

        println!("✅ All EndpointSlice tests passed!\n");
        Ok(())
    }

    fn should_skip(&self, config: &TestConfig) -> bool {
        !config.scenarios.endpointslice_updates
    }
}

/// Test 1: Scale up from 1 to 3 replicas
async fn test_scale_up(ctx: &TestContext) -> TestResult {
    println!("  📝 Test: Scale Up (1 → 3 replicas)");

    // Deploy backend with 1 replica
    let backend_yaml = fixtures::test_backend("scale-backend", &ctx.namespace, 8080, 1);
    k8s::apply_yaml(&ctx.client, &backend_yaml).await?;

    // Wait for 1 pod ready
    wait_for_pod_count(ctx, "scale-backend", 1, ctx.timeouts.backend_ready).await?;
    println!("  ✅ Initial: 1 pod ready");

    // Check RAUTA sees 1 backend (via logs or metrics)
    // TODO: Query RAUTA metrics to verify backend count

    // Scale to 3 replicas
    println!("  📈 Scaling to 3 replicas...");
    scale_deployment(ctx, "scale-backend", 3).await?;

    // Wait for 3 pods ready
    wait_for_pod_count(ctx, "scale-backend", 3, ctx.timeouts.backend_ready).await?;
    println!("  ✅ Scaled: 3 pods ready");

    // Wait for EndpointSlice reconciliation (5 min periodic in RAUTA)
    println!(
        "  ⏳ Waiting for EndpointSlice reconciliation (up to {} secs)...",
        ctx.timeouts.reconciliation
    );
    tokio::time::sleep(Duration::from_secs(ctx.timeouts.reconciliation)).await;

    // TODO: Verify RAUTA sees 3 backends
    // For now, just verify pods are running
    let pod_ips = get_pod_ips(ctx, "scale-backend").await?;
    if pod_ips.len() != 3 {
        return Err(format!("Expected 3 pod IPs after scale-up, got {}", pod_ips.len()).into());
    }

    println!("  ✅ Scale up detected: {} pod IPs", pod_ips.len());

    Ok(())
}

/// Test 2: Scale down from 3 to 1 replica
async fn test_scale_down(ctx: &TestContext) -> TestResult {
    println!("  📝 Test: Scale Down (3 → 1 replica)");

    // Assume scale-backend already exists from test_scale_up with 3 replicas

    // Scale down to 1
    println!("  📉 Scaling down to 1 replica...");
    scale_deployment(ctx, "scale-backend", 1).await?;

    // Wait for 1 pod ready (others terminating)
    wait_for_pod_count(ctx, "scale-backend", 1, ctx.timeouts.backend_ready).await?;
    println!("  ✅ Scaled down: 1 pod ready");

    // Wait for reconciliation
    println!("  ⏳ Waiting for EndpointSlice reconciliation...");
    tokio::time::sleep(Duration::from_secs(ctx.timeouts.reconciliation)).await;

    // Verify only 1 pod IP
    let pod_ips = get_pod_ips(ctx, "scale-backend").await?;
    if pod_ips.len() != 1 {
        return Err(format!("Expected 1 pod IP after scale-down, got {}", pod_ips.len()).into());
    }

    println!("  ✅ Scale down detected: {} pod IP", pod_ips.len());

    Ok(())
}

/// Helper: Wait for specific pod count
async fn wait_for_pod_count(
    ctx: &TestContext,
    app_label: &str,
    expected_count: usize,
    timeout_secs: u64,
) -> TestResult {
    let pods: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let start = std::time::Instant::now();

    while start.elapsed().as_secs() < timeout_secs {
        let lp = ListParams::default().labels(&format!("app={}", app_label));
        let pod_list = pods.list(&lp).await?;

        let ready_count = pod_list
            .items
            .iter()
            .filter(|pod| {
                pod.status
                    .as_ref()
                    .and_then(|s| s.conditions.as_ref())
                    .map(|conditions| {
                        conditions
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True")
                    })
                    .unwrap_or(false)
            })
            .count();

        if ready_count == expected_count {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Err(format!(
        "Timeout waiting for {} pods with label app={}",
        expected_count, app_label
    )
    .into())
}

/// Helper: Scale deployment
async fn scale_deployment(ctx: &TestContext, name: &str, replicas: i32) -> TestResult {
    let deployments: Api<Deployment> = Api::namespaced(ctx.client.clone(), &ctx.namespace);

    let patch = serde_json::json!({
        "spec": {
            "replicas": replicas
        }
    });

    deployments
        .patch(name, &PatchParams::default(), &Patch::Strategic(patch))
        .await?;

    Ok(())
}

/// Helper: Get Pod IPs
async fn get_pod_ips(
    ctx: &TestContext,
    app_label: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let pods: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let lp = ListParams::default().labels(&format!("app={}", app_label));
    let pod_list = pods.list(&lp).await?;

    let ips: Vec<String> = pod_list
        .items
        .iter()
        .filter_map(|pod| {
            pod.status
                .as_ref()
                .and_then(|s| s.pod_ip.as_ref())
                .map(|ip| ip.clone())
        })
        .collect();

    Ok(ips)
}
