use bollard::auth::DockerCredentials;
use bollard::image::CreateImageOptions;
use bollard::image::PruneImagesOptions;
use bollard::image::PushImageOptions;
use bollard::image::TagImageOptions;
use bollard::image::RemoveImageOptions;
use bollard::Docker;
use futures::stream::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::default::Default;
use std::env;
use tracing::event;
use tracing::Level;
use tracing_subscriber::fmt::format::FmtSpan;
use warp::hyper::StatusCode;
use warp::reject::Reject;
use warp::Filter;
use warp::Rejection;
use warp::Reply;

#[tokio::main]
async fn main() {
    // read Docker username from env
    let docker_username = env::var("USERNAME").unwrap_or_else(|e| {
        eprintln!("Failed to read Docker username: {}", e);
        std::process::exit(1);
    });

    // read Docker password fron env
    let docker_password = env::var("PASSWORD").unwrap_or_else(|e| {
        eprintln!("Failed to read Docker password: {}", e);
        std::process::exit(1);
    });

    let docker_username_filter = warp::any().map(move || docker_username.clone().into());
    let docker_password_filter = warp::any().map(move || docker_password.clone().into());

    // Filter traces based on the RUST_LOG env var, or, if it's not set,
    // default to show the output of the example.
    let filter = std::env::var("RUST_LOG").unwrap_or("tracing=info,warp=debug".to_owned());

    // Configure the default `tracing` subscriber.
    // The `fmt` subscriber from the `tracing-subscriber` crate logs `tracing`
    // events to stdout. Other subscribers are available for integrating with
    // distributed tracing systems such as OpenTelemetry.
    tracing_subscriber::fmt()
        // Use the filter we built above to determine which traces to record.
        .with_env_filter(filter)
        // Record an event when each span closes. This can be used to time our
        // routes' durations!
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let health = warp::get()
        .and(warp::path("health"))
        .and(warp::path::end())
        .and_then(health_check);

    let image_sync = warp::get()
        .and(warp::path("imagesync"))
        .and(warp::path::end())
        .and(warp::query())
        .and(docker_username_filter.clone())
        .and(docker_password_filter.clone())
        .and_then(sync_image);

    let prune_images = warp::get()
        .and(warp::path("prune_images"))
        .and(warp::path::end())
        .and(docker_username_filter.clone())
        .and(docker_password_filter.clone())
        .and_then(prune_images);

    let routes = image_sync
        .or(health)
        .or(prune_images)
        .with(warp::trace::request())
        .recover(return_error);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

#[derive(Debug)]
pub enum Error {
    ImageFormatError,
}

impl Reject for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::ImageFormatError => write!(f, "Image is null"),
        }
    }
}

#[tracing::instrument]
pub async fn return_error(r: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(crate::Error::ImageFormatError) = r.find() {
        Ok(warp::reply::with_status(
            "Image is null".to_string(),
            StatusCode::UNAUTHORIZED,
        ))
    } else {
        Ok(warp::reply::with_status(
            "Route not found".to_string(),
            StatusCode::NOT_FOUND,
        ))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SyncImageRes {
    pub source_image: String,
    pub dest_image: String,
}

#[tracing::instrument]
async fn health_check() -> Result<impl Reply, Rejection> {
    Ok(warp::reply::with_status("OK".to_string(), StatusCode::OK))
}

#[tracing::instrument]
async fn sync_image(
    map: HashMap<String, String>,
    username: String,
    password: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    // check request parameters
    if map.is_empty() {
        return Err(warp::reject::custom(Error::ImageFormatError));
    }

    let image = match map.get("image") {
        Some(value) => value,
        None => return Err(warp::reject::custom(Error::ImageFormatError)),
    };

    let mut parts: Vec<&str> = Vec::new();
    if image.contains('@') {
        parts.push(image);
    } else {
        parts = image.split(':').collect();
    }

    // length > 2
    if parts.len() > 2 {
        return Err(warp::reject::custom(Error::ImageFormatError));
    }

    // pull latest tag image
    if parts.len() == 1 {
        if !parts[0].contains("@") {
            parts.push("latest");
        }
    }

    let mut joined_image_str = parts.join(":");
    let mut tag_image_str = parts.join("_");

    // contain @ char
    if parts[0].contains("@") {
        joined_image_str = parts[0]
            .replace('/', "_")
            .replace('@', "_")
            .replace(':', "_")
            .to_string();
        tag_image_str = parts[0]
            .replace('/', "_")
            .replace('@', "_")
            .replace(':', "_")
            .to_string();
    }

    // create docker client
    let docker = Docker::connect_with_socket_defaults().unwrap();

    // create pull image options
    let mut pull_options = Some(CreateImageOptions {
        from_image: joined_image_str.clone(),
        ..Default::default()
    });

    if parts[0].contains("@") {
        pull_options = Some(CreateImageOptions {
            from_image: parts[0].to_string().clone(),
            ..Default::default()
        });
    }

    // create image stream
    let stream = docker.create_image(pull_options, None, None);

    // waiting pull image
    stream
        .for_each(|info| async {
            event!(Level::INFO, "{:?}", info.unwrap());
            // tracing::info!("{:?}", info.unwrap());
        })
        .await;
    event!(Level::INFO, "image pulled...");

    // create tag image options
    let tag_options = Some(TagImageOptions {
        repo: "dierbei/csi_demo",
        tag: &tag_image_str,
        // ..Default::default()
    });

    if parts[0].contains("@") {
        // playing image tag
        let _ret = docker.tag_image(&parts[0], tag_options).await;
        event!(Level::INFO, "played image tag...");
    } else {
        // playing image tag
        let _ret = docker.tag_image(&joined_image_str, tag_options).await;
        event!(Level::INFO, "played image tag...");
    }

    // create docker credentials
    let credentials = Some(DockerCredentials {
        username: Some(username.to_string()),
        password: Some(password.to_string()),
        ..Default::default()
    });

    // create push image options
    let push_options = Some(PushImageOptions {
        tag: &tag_image_str,
    });

    // create push image steam
    let stream = docker.push_image("dierbei/csi_demo", push_options, credentials);

    // pushing image
    stream
        .for_each(|l| async {
            event!(Level::INFO, "{:?}", l.unwrap());
        })
        .await;

    let remove_source_options = Some(RemoveImageOptions {
        force: true,
        ..Default::default()
    });
    
    let _resp = match docker.remove_image(&joined_image_str, remove_source_options, None).await {
        Ok(r) => r,
        Err(e) => {
            event!(Level::ERROR, "{:?}", e);
            return Err(warp::reject::custom(Error::ImageFormatError));
        }
    };

    let remove_dst_options = Some(RemoveImageOptions {
        force: true,
        ..Default::default()
    });

    let _resp = match docker.remove_image(&format!("dierbei/csi_demo:{}", tag_image_str), remove_dst_options, None).await {
        Ok(r) => r,
        Err(e) => {
            event!(Level::ERROR, "{:?}", e);
            return Err(warp::reject::custom(Error::ImageFormatError));
        }
    };

    Ok(warp::reply::json(&SyncImageRes {
        source_image: joined_image_str.clone(),
        dest_image: tag_image_str.clone(),
    }))
}

#[tracing::instrument]
async fn prune_images(
    username: String,
    password: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    // create docker client
    let docker = Docker::connect_with_socket_defaults().unwrap();

    let mut filters = HashMap::new();
    filters.insert("until", vec!["1m"]);

    let options = Some(PruneImagesOptions { filters });

    let resp = match docker.prune_images(options).await {
        Ok(r) => r,
        Err(e) => {
            event!(Level::ERROR, "{:?}", e);
            return Err(warp::reject::custom(Error::ImageFormatError));
        }
    };

    Ok(warp::reply::json(&resp))
}

// async fn list_image() -> Result<()> {
//     let docker = Docker::connect_with_socket_defaults()?;
//     let images = docker
//         .list_images(Some(ListImagesOptions::<String> {
//             all: true,
//             ..Default::default()
//         }))
//         .await
//         .unwrap();

//     for image in images {
//         println!("id: {}, tags: {:?}", image.id, image.repo_tags);
//     }

//     Ok(())
// }
