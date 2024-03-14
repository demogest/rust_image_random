use actix_multipart::Multipart;
use actix_web::{web, Error, HttpRequest, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use image::{io::Reader as ImageReader, ImageFormat};
use md5::{Digest, Md5};
use rand::Rng;
use serde_json::Value;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::utils::*;


// Get the specified image
#[actix_web::get("/api/image/{filename}")]
pub async fn get_image(
    filename: web::Path<String>,
    data: web::Data<Vec<Vec<String>>>,
    req: HttpRequest,
) -> impl Responder {
    let filename = filename.into_inner();
    let pc_images = &data[0];
    let mp_images = &data[1];
    let file_path = pc_images
        .iter()
        .chain(mp_images.iter())
        .find(|&path| {
            Path::new(path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
                == filename
        })
        .map(|path| path);
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
        "Visitor IP: {}, Country: {}, file: {}",
        ip_str, country, filename
    );
    
    if let Some(file_path) = file_path {
        let mut file = File::open(file_path).unwrap();
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).unwrap();
        HttpResponse::Ok().content_type("image/jpeg").body(buffer)
    } else {
        HttpResponse::NotFound().json(Value::String("Image not found.".to_string()))
    }
}

// Get file list
#[actix_web::get("/api/list/{subfolder}")]
pub async fn get_list(
    subfolder: web::Path<String>,
    data: web::Data<Vec<Vec<String>>>,
) -> impl Responder {
    let subfolder = subfolder.into_inner();
    let pc_images = &data[0];
    let mp_images = &data[1];
    let filtered_images: Vec<&String> = if subfolder == "all" {
        pc_images.iter().chain(mp_images.iter()).collect()
    } else if subfolder == "pc" {
        pc_images.iter().collect()
    } else if subfolder == "mp" {
        mp_images.iter().collect()
    } else {
        return HttpResponse::NotFound().json(Value::String("Invalid subfolder.".to_string()));
    };
        

    if filtered_images.is_empty() {
        return HttpResponse::NotFound().json(Value::String("No images found.".to_string()));
    }

    let mut file_list = Vec::new();
    for image in filtered_images {
        // Only return filename
        let file_name = Path::new(image)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        file_list.push(file_name);
    }

    HttpResponse::Ok().json(file_list)
}

#[actix_web::post("/api/images/{subfolder}")]
pub async fn upload_image(
    mut payload: Multipart,
    subfolder: web::Path<String>,
    data: web::Data<Vec<Vec<String>>>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let token = &data[2][1];
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
    if auth_header
        != Some(
            &actix_web::http::header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        )
    {
        println!(
            "Unauthorized access from IP: {}, Country: {}",
            ip_str, country
        );
        return Err(actix_web::error::ErrorUnauthorized("Unauthorized."));
    }
    // Get the folder path from the data
    let image_folder = &data[2][0];
    let mut filepaths: Vec<String> = Vec::new();
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
        match img.save_with_format(new_filepath.clone(), ImageFormat::WebP) {
            Ok(_) => {
                filepaths.push("/api/image/".to_owned()+new_filename.as_str());

                println!("Image uploaded from {} saved to {}",ip_str, new_filepath);
                match create_thumbnail(Path::new(&new_filepath), 200, 200, &image_folder) {
                    Ok(_) => {
                        println!("Created thumbnail for {new_filepath}");
                    }
                    Err(e) => {
                        eprintln!("Failed to create thumbnail: {e}");
                        return Err(actix_web::error::ErrorInternalServerError(
                            "Image uploaded successfully, but failed to create thumbnail.",
                        ));
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to save image: {}", e);
                return Err(actix_web::error::ErrorInternalServerError(
                    "Failed to save image.",
                ));
            }
        }
    }
    Ok(HttpResponse::Ok().json(filepaths))
}

#[actix_web::get("/api/images/{subfolder}")]
pub async fn list_images(
    subfolder: web::Path<String>,
    data: web::Data<Vec<Vec<String>>>,
    req: HttpRequest,
) -> impl Responder {
    let pc_images = &data[0];
    let mp_images = &data[1];
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
    let filtered_images: Vec<&String> = if subfolder == "pc" {
        pc_images.iter().collect()
    } else if subfolder == "mp" {
        mp_images.iter().collect()
    } else if subfolder == "all" {
        pc_images.iter().chain(mp_images.iter()).collect()
    } else {
        return HttpResponse::NotFound().json(Value::String("Invalid subfolder.".to_string()));
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

// Get the specified thumbnail
#[actix_web::get("/api/thumbnail/{filename}")]
pub async fn get_thumbnail(
    filename: web::Path<String>,
    data: web::Data<Vec<Vec<String>>>,
) -> impl Responder {
    let filename = filename.into_inner();
    let img_folder = &data[2][0];
    let mut file = File::open(format!("{}/thumbnails/{}", img_folder, filename)).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();
    HttpResponse::Ok().content_type("image/jpeg").body(buffer)
}