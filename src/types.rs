use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Applet {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Applets {
    pub list: Vec<Applet>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Application {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Applications {
    pub list: Vec<Application>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Themes {
    pub list: Vec<Theme>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Services {
    pub list: Vec<Service>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scripts {
    pub list: Vec<Script>,
}
