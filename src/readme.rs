use crate::types::{Applets, Applications, Scripts, Services, Themes};

pub fn generate() -> std::io::Result<()> {
    let mut readme = String::new();
    let applications: Applications = ron::from_str(include_str!("../applications.ron")).unwrap();
    let applets: Applets = ron::from_str(include_str!("../applets.ron")).unwrap();
    let themes: Themes = ron::from_str(include_str!("../themes.ron")).unwrap();
    let services: Services = ron::from_str(include_str!("../services.ron")).unwrap();
    let scripts: Scripts = ron::from_str(include_str!("../scripts.ron")).unwrap();

    readme.push_str("# COSMIC Project Collection\n");
    readme.push_str("A collection of COSMIC projects developed by the community.\n\n");

    write_applications(&mut readme, applications);
    write_applets(&mut readme, applets);

    write_services(&mut readme, services);
    write_themes(&mut readme, themes);
    write_scripts(&mut readme, scripts);

    readme.push_str("\n## How to add your project?\n");
    readme.push_str("To add your project to this list, please open a pull request with your project added to the `applications.ron` or `applets.ron` file.\n");

    std::fs::write("README.md", readme)
}

fn write_applications(readme: &mut String, applications: Applications) {
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
}

fn write_applets(readme: &mut String, applets: Applets) {
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
}

fn write_themes(readme: &mut String, themes: Themes) {
    readme.push_str("\n## Themes\n");

    readme.push_str("| Name | Description | Image |\n");
    readme.push_str("|---|---|---|\n");

    for applet in themes.list {
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
}

fn write_services(readme: &mut String, services: Services) {
    readme.push_str("\n## Services\n");

    readme.push_str("| Name | Description | Image |\n");
    readme.push_str("|---|---|---|\n");

    for service in services.list {
        let mut row = String::new();

        row.push_str(&format!(
            "| [{}]({}) | {} |",
            service.name, service.repo, service.description
        ));

        match service.image {
            Some(image) => {
                row.push_str(&format!(
                    " <img src=\"{}\" alt=\"{}\" width=\"200\"/> |\n",
                    image, service.name
                ));
            }
            None => row.push_str(" |\n"),
        }

        readme.push_str(&row);
    }
}

fn write_scripts(readme: &mut String, scripts: Scripts) {
    readme.push_str("\n## Scripts\n");

    readme.push_str("| Name | Description | Image |\n");
    readme.push_str("|---|---|---|\n");

    for script in scripts.list {
        let mut row = String::new();

        row.push_str(&format!(
            "| [{}]({}) | {} |",
            script.name, script.repo, script.description
        ));

        match script.image {
            Some(image) => {
                row.push_str(&format!(
                    " <img src=\"{}\" alt=\"{}\" width=\"200\"/> |\n",
                    image, script.name
                ));
            }
            None => row.push_str(" |\n"),
        }

        readme.push_str(&row);
    }
}
