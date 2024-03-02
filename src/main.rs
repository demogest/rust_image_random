use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    host: String,
    port: u16,
    image_folder: String,
}

fn read_config(config_file: &str) -> Config {
    match File::open(config_file) {
        Ok(file) => {
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap()
        }
        Err(_) => {
            // 当找不到文件时，创建一个默认配置
            let default_config = Config {
                host: "127.0.0.1".to_string(),
                port: 8080,
                image_folder: "./images".to_string(),
            };
            let serialized = serde_json::to_string_pretty(&default_config).unwrap();
            let mut file = File::create(config_file).expect("Unable to create config file");
            file.write_all(serialized.as_bytes())
                .expect("Unable to write to config file");
            println!("Default config created: {}", config_file);
            default_config
        }
    }
}

fn index_images(folder: &str) -> Vec<String> {
    // 创建一个Path实例
    let path = Path::new(folder);

    // 检查路径是否存在
    if !path.exists() {
        println!("The folder '{}' does not exist.", folder);
        return Vec::new(); // 返回一个空的Vec
    }

    // 检查路径是否是一个目录
    if !path.is_dir() {
        println!("The path '{}' is not a directory.", folder);
        return Vec::new(); // 返回一个空的Vec
    }

    // 检查目录是否为空
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                println!("The directory '{}' is empty.", folder);
                return Vec::new(); // 返回一个空的Vec
            }
        }
        Err(e) => {
            println!("Failed to read the directory '{}': {}", folder, e);
            return Vec::new(); // 返回一个空的Vec
        }
    }

    // 如果目录存在且不为空，则继续索引图片
    WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("")
                == "jpg"
        })
        .map(|e| e.path().to_str().unwrap().to_string())
        .collect()
}

async fn list_images(subfolder: web::Path<String>, data: web::Data<Vec<String>>, req: HttpRequest) -> impl Responder {
    // Get the visitor's ip address and print to log
    if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        if let Ok(ip_str) = cf_ip.to_str() {
            println!(
                "Visitor IP (from Cloudflare): {}, subfolder: {}",
                ip_str, subfolder
            );
        }
    } else if let Some(peer_addr) = req.peer_addr() {
        println!("Visitor IP: {}, subfolder: {}", peer_addr.ip(), subfolder);
    } else {
        println!("Could not determine visitor IP., subfolder: {}", subfolder);
    }
    let subfolder = subfolder.into_inner();
    let filtered_images: Vec<&String> = if subfolder == "all" {
        data.iter().collect()
    } else {
        data.iter()
            .filter(|&path| path.contains(&subfolder))
            .collect()
    };

    if filtered_images.is_empty() {
        return HttpResponse::NotFound().json(Value::String("No images found.".to_string()));
    }

    let mut rng = rand::thread_rng();
    let random_index = rng.gen_range(0..filtered_images.len());
    let random_image = filtered_images.get(random_index).unwrap();

    let mut file = File::open(random_image).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();

    HttpResponse::Ok().content_type("image/jpeg").body(buffer)
}

#[actix_web::main] // <- Start actix-web
async fn main() -> std::io::Result<()> {
    let config = read_config("config.json");

    // Print the config
    println!("Config: {:?}", config);

    let images = index_images(&config.image_folder);

    // Print the number of images indexed
    println!("Indexed {} images.", images.len());

    // Attempt to bind the server to the provided address
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(images.clone()))
            .route("/", web::get().to(|| async { "Hello, world!" }))
            .route("/api/images/{subfolder}", web::get().to(list_images))
    })
    .bind(format!("{}:{}", config.host, config.port));

    // Check if the server was successfully bound
    match server {
        Ok(server) => {
            println!("Server running at http://{}:{}", config.host, config.port); // Print a success message
            server.run().await // Start the server
        }
        Err(e) => {
            println!("Failed to bind server: {}", e); // Print an error message
            std::process::exit(1); // Exit the program
        }
    }
}
