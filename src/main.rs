use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Applet {
    name: String,
    description: String,
    repo: String,
    image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Applets {
    list: Vec<Applet>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Application {
    name: String,
    description: String,
    repo: String,
    image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Applications {
    list: Vec<Application>,
}

fn main() -> anyhow::Result<()> {
    let applications_data = include_str!("../applications.ron");
    let applets_data = include_str!("../applets.ron");
    let mut readme = String::new();
    let applications: Applications = ron::from_str(applications_data).unwrap();
    let applets: Applets = ron::from_str(applets_data).unwrap();

    readme.push_str("# COSMIC Project Collection\n");
    readme.push_str("A collection of COSMIC projects developed by the community.\n\n");

    readme.push_str("## Applications\n");

    readme.push_str("| Name | Description | Image |\n");
    readme.push_str("|---|---|---|\n");

    for app in applications.list {
        let mut row = String::new();

        row.push_str(&format!(
            "| [{}]({}) | {} |",
            app.name, app.repo, app.description
        ));

        match app.image {
            Some(image) => {
                row.push_str(&format!(
                    " <img src=\"{}\" alt=\"{}\" width=\"200\"/> |\n",
                    image, app.name
                ));
            }
            None => row.push_str(" |\n"),
        }

        readme.push_str(&row);
    }

    readme.push_str("\n## Applets\n");

    readme.push_str("| Name | Description | Image |\n");
    readme.push_str("|---|---|---|\n");

    for applet in applets.list {
        let mut row = String::new();

        row.push_str(&format!(
            "| [{}]({}) | {} |",
            applet.name, applet.repo, applet.description
        ));

        match applet.image {
            Some(image) => {
                row.push_str(&format!(
                    " <img src=\"{}\" alt=\"{}\" width=\"200\"/> |\n",
                    image, applet.name
                ));
            }
            None => row.push_str(" |\n"),
        }

        readme.push_str(&row);
    }

    readme.push_str("\n## How to add your project?\n");
    readme.push_str("To add your project to this list, please open a pull request with your project added to the `applications.ron` or `applets.ron` file.\n");

    std::fs::write("README.md", readme)?;

    Ok(())
}
