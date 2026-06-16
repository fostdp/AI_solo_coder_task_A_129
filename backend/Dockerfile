# ============================================================
# Stage 1: 构建 Rust 静态二进制
# ============================================================
FROM rust:1.77-alpine3.19 AS builder

# 安装编译依赖（alpine 下的 musl-tools 和静态 openssl）
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    perl \
    make \
    gcc \
    libc-dev

# 工作目录
WORKDIR /usr/src/bronze-drum

# 创建空的 main.rs 用于依赖缓存
RUN cargo init --bin --name bronze-drum-server

# 先复制 Cargo.toml 和 Cargo.lock 以便缓存依赖层
COPY Cargo.toml Cargo.lock ./

# 构建依赖（仅依赖层，加速后续构建）
RUN cargo build --release 2>/dev/null || true

# 复制真实源码和配置
COPY src ./src
COPY config ./config

# 删除空的 main.rs 缓存目标，触发重新构建
RUN touch src/main.rs

# 静态链接 OpenSSL 和 musl libc，构建 release 二进制
ENV OPENSSL_STATIC=1
ENV PKG_CONFIG_ALL_STATIC=1
ENV RUSTFLAGS="-C target-feature=+crt-static"

RUN cargo build --release

# ============================================================
# Stage 2: 运行时镜像（最小化，仅含二进制 + 配置 + CA 证书）
# ============================================================
FROM alpine:3.19 AS runtime

# 安装运行时最小依赖
RUN apk add --no-cache \
    ca-certificates \
    tzdata \
    curl

# 设置时区
ENV TZ=Asia/Shanghai

# 非 root 用户，提升安全性
RUN addgroup -S bronze && adduser -S bronze -G bronze

# 工作目录
WORKDIR /app

# 从 builder 拷贝二进制和配置
COPY --from=builder /usr/src/bronze-drum/target/release/bronze-drum-server /app/bronze-drum-server
COPY --from=builder /usr/src/bronze-drum/config /app/config

# 设置文件权限
RUN chmod +x /app/bronze-drum-server && \
    chown -R bronze:bronze /app

USER bronze

# 配置查找路径
ENV CONFIG_DIR=/app/config

# 暴露端口：HTTP API + Prometheus metrics
EXPOSE 8080 9090

# 健康检查
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:8080/api/health || exit 1

# 启动命令
CMD ["/app/bronze-drum-server"]
