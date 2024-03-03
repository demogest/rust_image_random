use actix_multipart::Multipart;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use futures::{StreamExt, TryStreamExt};
use image::{io::Reader as ImageReader, ImageFormat};
use md5::{Digest, Md5};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use base64::prelude::*;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    host: String,
    port: u16,
    image_folder: String,
    pwd: String,
}

fn read_config(config_file: &str) -> Config {
    match File::open(config_file) {
        Ok(file) => {
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap()
        }
        Err(_) => {
            // Dump the default config to the file
            let default_config = Config {
                host: "127.0.0.1".to_string(),
                port: 8080,
                image_folder: "./images".to_string(),
                pwd: "secret".to_string(),
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
    if !Path::new(&format!("{}/pc", folder)).exists()
        || !Path::new(&format!("{}/mp", folder)).exists()
    {
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

#[actix_web::post("/api/images/{subfolder}")]
async fn upload_image(
    mut payload: Multipart,
    subfolder: web::Path<String>,
    data: web::Data<Vec<String>>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let token = data[data.len() - 1].clone();
    // Record the ip address of the visitor
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // Convert to String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "".to_string() // Could not get the ip address
    };
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };
    // Check authentication, should be Bearer <token>
    let auth_header = req.headers().get("Authorization");
    if auth_header != Some(&actix_web::http::header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap()) {
        println!("Unauthorized access from IP: {}, Country: {}", ip_str, country);
        return Err(actix_web::error::ErrorUnauthorized("Unauthorized."));
    }
    // Get the folder path from the data
    let image_folder = data[data.len() - 2].clone();
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_disposition = field.content_disposition();
        let filename = match content_disposition.get_filename() {
            Some(name) => name,
            None => return Err(actix_web::error::ErrorBadRequest("No filename found.")),
        };
        // Get the subfolder from the path as a string
        let subfolder = subfolder.clone();
        let folder_path = format!("{}/{}", image_folder, subfolder);

        fs::create_dir_all(&folder_path)?;

        let mut hasher = Md5::new();
        hasher.update(filename.as_bytes());
        let hash_result = hasher.finalize();
        let new_filename = format!("{:x}.webp", hash_result);
        let new_filepath = format!("{}/{}", folder_path, new_filename);

        let mut buffer = Vec::new();
        // Read the data from the field
        while let Some(chunk) = field.next().await {
            let data = chunk?;
            buffer.extend_from_slice(&data);
        }

        // Load the image from the buffer
        let img = ImageReader::new(std::io::Cursor::new(buffer))
            .with_guessed_format()
            .expect("Failed to guess image format")
            .decode()
            .expect("Failed to decode image");

        // Save the image to the file
        img.save_with_format(new_filepath, ImageFormat::WebP)
            .expect("Failed to save image");

        println!("Image uploaded from IP: {}, Country: {}", ip_str, country);
    }
    Ok(HttpResponse::Ok().json("Images uploaded successfully."))
}

#[actix_web::get("/api/images/{subfolder}")]
async fn list_images(
    subfolder: web::Path<String>,
    data: web::Data<Vec<String>>,
    req: HttpRequest,
) -> impl Responder {
    let data = data[0..data.len() - 2].to_vec();
    // Get the visitor's ip address and print to log
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // Convert to String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "".to_string() // Could not get the ip address
    };

    // Get the visitor's country and print to log
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };

    println!(
        "Visitor IP: {}, Country: {}, Subfolder: {}",
        ip_str, country, subfolder
    );

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

    let image_folder = config.image_folder.clone();
    let mut data_vec = images.clone();
    let token = BASE64_STANDARD.encode(config.pwd.as_bytes());
    data_vec.push(image_folder);
    data_vec.push(token);

    // Attempt to bind the server to the provided address
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(data_vec.clone()))
            .route("/", web::get().to(|| async { "Hello, world!" }))
            .service(list_images)
            .service(upload_image)
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
