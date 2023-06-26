## 简介
image-sync 是一个用于解决拉取国外镜像失败的工具。它提供了一个简单而高效的方式来同步并拉取国外镜像，并将其推送到可访问的 Docker Hub。

## 快速开始
```shell
USERNAME=<docker-username> PASSWORD=<docker-password> cargo run
```

## 核心功能
MirrorSync 的核心功能包括：

镜像拉取：MirrorSync 提供了简单而直观的界面，允许用户根据自己的需求选择特定的镜像进行拉取。

镜像推送：一旦镜像成功拉取到本地，image-sync 提供了将镜像推送到 Docker Hub。用户可以轻松分享自己的镜像或在不同的环境中使用它们。

## 编译问题
- https://docs.rs/tokio/latest/tokio/

## 参考项目
- https://docs.rs/bollard/latest/bollard/struct.Docker.html#