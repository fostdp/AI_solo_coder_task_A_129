# ============================================================
# 前端：Nginx 提供静态资源 + Gzip 压缩 + API/metrics 反向代理
# ============================================================
FROM nginx:1.27-alpine3.19-slim

# 替换默认 Nginx 配置（含 Gzip 压缩）
COPY nginx.conf /etc/nginx/nginx.conf

# 删除默认页面
RUN rm -rf /usr/share/nginx/html/*

# 复制前端静态资源
COPY index.html /usr/share/nginx/html/
COPY js /usr/share/nginx/html/js/

# 健康检查：通过前端容器反代到后端 /api/health
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -qO- http://localhost/api/health >/dev/null 2>&1 || exit 1

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
