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
            // Create the folder if it doesn't exist
            fs::create_dir_all("./images/pc").expect("Unable to create image folder for pc");
            fs::create_dir_all("./images/mp").expect("Unable to create image folder for mp");
            file.write_all(serialized.as_bytes())
                .expect("Unable to write to config file");
            println!("Default config created: {}", config_file);
            default_config
        }
    }
}

// Convert the image to webp format
fn convert_images_to_webp(folder_path: &str) -> std::io::Result<usize> {
    let mut converted_count = 0;
    // Recursively iterate through the folder
    for entry in fs::read_dir(folder_path)? {
        let entry = entry?;
        let path = entry.path();

        // Check if the path is a file
        if path.is_file() {
            // Filter the files by extension
            if let Some(ext) = path.extension() {
                if ext == "jpg" || ext == "png" || ext == "jpeg" {
                    // Try to open the image
                    match image::open(&path) {
                        Ok(img) => {
                            // Create a new path with the same name but with the webp extension
                            let new_path = path.with_extension("webp");

                            // Save the image in webp format
                            match img.save_with_format(new_path, image::ImageFormat::WebP) {
                                Ok(_) => {
                                    // Remove the original image
                                    fs::remove_file(&path)?;
                                    println!("Converted {:?} to webp.", path);
                                    converted_count += 1;
                                }
                                Err(e) => eprintln!("Failed to convert {:?}: {}", path, e),
                            }
                        }
                        Err(e) => eprintln!("Failed to open {:?}: {}", path, e),
                    }
                }
            }
        } else if path.is_dir() {
            // If the path is a directory, call the function recursively
            converted_count += convert_images_to_webp(&path.to_str().unwrap())?;
        }
    }
    Ok(converted_count)
}

fn index_images(folder: &str) -> Vec<String> {
    // Check if the folder exists
    if !Path::new(folder).exists() {
        panic!("Image folder does not exist: {}", folder);
    }
    // Validate the structure of the folder, the folder should contain subfolders 'pc' and 'mp'
    if !Path::new(&format!("{}/pc", folder)).exists() || !Path::new(&format!("{}/mp", folder)).exists() {
        panic!("Image folder structure is invalid. It should contain subfolders 'pc' and 'mp'.");
    }

    // If the folder exists, index the images
    WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("")
                == "webp"
        })
        .map(|e| e.path().to_str().unwrap().to_string())
        .collect()
}

#[actix_web::get("/api/images/{subfolder}")]
async fn list_images(subfolder: web::Path<String>, data: web::Data<Vec<String>>, req: HttpRequest) -> impl Responder {
    // Get the visitor's ip address and print to log
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // 转换为String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string() // 已经是String，不需要再次转换
    } else {
        "".to_string() // 转换为空String
    };
    
    // 由于ip_str现在是String类型，我们在传递给get_country_by_ip时需要传递引用
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };

    println!("Visitor IP: {}, Country: {}, Subfolder: {}", ip_str, country, subfolder);

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

    // Convert the images to webp format
    match convert_images_to_webp(&config.image_folder) {
        Ok(count) => println!("{} images converted to webp.", count),
        Err(e) => eprintln!("Failed to convert images: {}", e),
    }

    let images = index_images(&config.image_folder);

    // Print the number of images indexed
    println!("Indexed {} images.", images.len());

    // Attempt to bind the server to the provided address
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(images.clone()))
            .route("/", web::get().to(|| async { "Hello, world!" }))
            .service(list_images)
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
