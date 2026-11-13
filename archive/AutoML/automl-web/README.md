# AutoML Web

这是 Auto.ml 研究平台的 React + Vite 前端。生产环境由 Rust `automl-server` 把 `dist/` 构建产物嵌入并通过 `https://auto.ml/` 对外服务。

## 本地开发

```sh
npm ci
npm run dev
```

前端通过 `/api/v1/*` 调用 Rust API。本地开发时可以让 `automl-server` 运行在 `127.0.0.1:3100`，必要时在 Vite 代理里指向该后端。生产交付时，`automl-server.service` 在 `127.0.0.1:8030` 监听，前置共享 Caddy 路由为 `https://auto.ml/`。

## OpenAPI

接口事实来源由后端暴露：

- `https://auto.ml/openapi.json`
- `https://auto.ml/api/openapi.json`

前端代码不单独维护接口清单；页面、组件和 API 调用应以 Rust 后端的 OpenAPI 文档和 `/api/v1/*` 实际路由为准。

## 验证

```sh
npm run lint
npm run build
```

这个源码树不包含项目本地 Worker 配置。旧的 `auto.ml` Cloudflare Worker 只作为 `automl-web-worker` 记录切换历史；当前目标运行时是 Rust 服务，原运营部署配置不在此公开源码分发中。
