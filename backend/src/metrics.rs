//! Prometheus 指标采集模块
//! 暴露 /metrics 端点，供 Prometheus 抓取
//! 指标分类：HTTP、传感器、铸造仿真、声学分析、告警

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::Arc;
use std::time::Instant;

/// 全局指标句柄（用于自定义采集）
pub struct MetricsRegistry {
    pub handle: PrometheusHandle,
    pub start_time: Instant,
}

/// 初始化 Prometheus 导出器，绑定到指定地址
pub fn init_prometheus() -> Arc<MetricsRegistry> {
    let builder = PrometheusBuilder::new();

    // 关键指标定义（通过 metrics crate 的宏注册，首次调用时自动创建）
    // 这里先预设描述信息

    let handle = builder.install_recorder().expect("Failed to install Prometheus recorder");

    // 注册全局描述（可选，帮助 Prometheus 识别）
    metrics::describe_counter!(
        "bronze_http_requests_total",
        "Total HTTP requests by endpoint and method",
    );
    metrics::describe_histogram!(
        "bronze_http_request_duration_seconds",
        "HTTP request latency in seconds",
    );
    metrics::describe_counter!(
        "bronze_sensor_readings_total",
        "Total sensor readings received by drum_id",
    );
    metrics::describe_counter!(
        "bronze_casting_simulations_total",
        "Total casting simulations by drum_id",
    );
    metrics::describe_counter!(
        "bronze_acoustic_analyses_total",
        "Total acoustic analyses by drum_id",
    );
    metrics::describe_gauge!(
        "bronze_alarms_pending",
        "Number of pending alarms awaiting flush",
    );
    metrics::describe_counter!(
        "bronze_alarms_total",
        "Total alarms triggered by severity",
    );
    metrics::describe_gauge!(
        "bronze_active_sessions",
        "Number of active drum sessions",
    );
    metrics::describe_gauge!(
        "bronze_build_info",
        "Build information",
    );

    Arc::new(MetricsRegistry {
        handle,
        start_time: Instant::now(),
    })
}

/// 渲染 Prometheus 文本格式的指标输出
pub fn render(registry: &MetricsRegistry) -> String {
    let mut output = registry.handle.render();

    // 添加自定义指标：进程启动时间
    let uptime = registry.start_time.elapsed().as_secs_f64();
    output.push_str(&format!(
        "# HELP bronze_process_uptime_seconds Process uptime in seconds.\n\
         # TYPE bronze_process_uptime_seconds gauge\n\
         bronze_process_uptime_seconds {uptime:.3}\n"
    ));

    // 构建信息
    output.push_str(
        "# HELP bronze_build_info Build info and version.\n\
         # TYPE bronze_build_info gauge\n\
         bronze_build_info{version=\"2.0.0\",arch=\"microservices-mpsc\"} 1\n"
    );

    output
}

// ============ 便捷宏：计数器递增 ============

#[inline]
pub fn inc_http_request(method: &str, path: &str, status: u16) {
    metrics::counter!(
        "bronze_http_requests_total",
        "method" => method.to_string(),
        "path" => path.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);
}

#[inline]
pub fn record_http_duration(method: &str, path: &str, secs: f64) {
    metrics::histogram!(
        "bronze_http_request_duration_seconds",
        "method" => method.to_string(),
        "path" => path.to_string(),
    )
    .record(secs);
}

#[inline]
pub fn inc_sensor_reading(drum_id: &str, valid: bool) {
    metrics::counter!(
        "bronze_sensor_readings_total",
        "drum_id" => drum_id.to_string(),
        "valid" => if valid { "true" } else { "false" }.to_string(),
    )
    .increment(1);
}

#[inline]
pub fn inc_casting_simulation(drum_id: &str, quality_ok: bool) {
    metrics::counter!(
        "bronze_casting_simulations_total",
        "drum_id" => drum_id.to_string(),
        "quality_ok" => if quality_ok { "true" } else { "false" }.to_string(),
    )
    .increment(1);
}

#[inline]
pub fn inc_acoustic_analysis(drum_id: &str, pass: bool) {
    metrics::counter!(
        "bronze_acoustic_analyses_total",
        "drum_id" => drum_id.to_string(),
        "pass" => if pass { "true" } else { "false" }.to_string(),
    )
    .increment(1);
}

#[inline]
pub fn set_pending_alarms(count: usize) {
    metrics::gauge!("bronze_alarms_pending").set(count as f64);
}

#[inline]
pub fn inc_alarm(severity: &str, alarm_type: &str) {
    metrics::counter!(
        "bronze_alarms_total",
        "severity" => severity.to_string(),
        "type" => alarm_type.to_string(),
    )
    .increment(1);
}

#[inline]
pub fn set_active_sessions(count: usize) {
    metrics::gauge!("bronze_active_sessions").set(count as f64);
}
