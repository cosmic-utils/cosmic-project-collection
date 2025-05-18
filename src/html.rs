use crate::types::*;

pub fn generate() -> std::io::Result<()> {
    let mut html = String::new();
    let applications: Applications = ron::from_str(include_str!("../applications.ron")).unwrap();
    let applets: Applets = ron::from_str(include_str!("../applets.ron")).unwrap();
    let themes: Themes = ron::from_str(include_str!("../themes.ron")).unwrap();
    let services: Services = ron::from_str(include_str!("../services.ron")).unwrap();
    let scripts: Scripts = ron::from_str(include_str!("../scripts.ron")).unwrap();

    html.push_str(r#"<!DOCTYPE html>
<html lang="en">

<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>COSMIC Project Collection</title>
    <link rel="icon" type="image/x-icon" href="https://cdn11.bigcommerce.com/s-pywjnxrcr2/images/stencil/original/image-manager/znbla5m069vx10kz-cosmic-sectiontutorial-orange.png?t=1723672482">
    <script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
</head>

<body>
    <div
        class="min-h-screen bg-gradient-to-br from-zinc-300 via-gray-300 to-zinc-300 dark:from-neutral-900 dark:via-zinc-900 dark:to-neutral-900 flex flex-col items-center px-4 py-16">
        <header class="mb-12 text-center">
            <img class="h-80 hidden dark:block"
                src="https://raw.githubusercontent.com/system76/brand/bed1fd3fd39d3f3371c57f15878195888c617777/COSMIC%20branding/cosmic%20logo%20white%20+%20gradient%20full.svg"
                alt="" />
            <img class="h-80 block dark:hidden"
                src="https://raw.githubusercontent.com/system76/brand/bed1fd3fd39d3f3371c57f15878195888c617777/COSMIC%20branding/cosmic%20logo%20neutral%20+%20gradient%20full.svg"
                alt="" />
            <h1 class="text-5xl font-extrabold text-zinc-800 dark:text-white mb-4">
                Project Collection
            </h1>
            <p class="text-xl text-zinc-800 dark:text-white max-w-2xl mx-auto">
                Explore a curated selection of innovative projects, experiments, and
                creative works from the COSMIC &trade; community.
            </p>
        </header>
        <div class="grid gap-8 grid-cols-1 md:grid-cols-2 lg:grid-cols-3 w-full max-w-6xl">
            "#);
    write_applications(&mut html, applications);
    write_applets(&mut html, applets);
    write_themes(&mut html, themes);
    write_services(&mut html, services);
    write_scripts(&mut html, scripts);

    html.push_str(
        r#"
    </div>
        <footer class="mt-16 text-zinc-800 dark:text-white text-sm">
            &copy; 2025 COSMIC &trade; Project Collection. All rights reserved.
        </footer>
    </div>
</body>

</html>
"#,
    );

    std::fs::write("docs/index.html", html)
}

fn write_applications(html: &mut String, applications: Applications) {
    html.push_str(
        r#"<div class="col-span-full mb-2">
                <h2 class="text-3xl font-bold text-zinc-800 dark:text-white mb-4">
                    Applications
                </h2>
            </div>
            "#,
    );
    for project in applications.list {
        html.push_str(&format!(
            r#"<div class="bg-white/50 dark:bg-neutral-800 rounded-xl shadow-lg p-6 flex flex-col items-start hover:scale-105 transition-transform">
                {}
                <h2 class="text-2xl font-bold text-zinc-800 dark:text-white mb-2">{}</h2>
                <p class="text-zinc-800 dark:text-white mb-4">{}</p>
                <a href="{}" class="mt-auto text-zinc-800 dark:text-white font-semibold hover:underline">View Project →</a>
            </div>
            "#,
            match project.image {
                Some(img) => format!(r#"<a href="{}" class="w-full h-50 object-cover rounded-lg mb-4">
                    <img src="{}" alt="{}" class="w-full h-50 object-cover rounded-lg mb-4"/>
                </a>"#, img, img, project.name),
                None => "".to_string(),
            },
            project.name,
            project.description,
            project.repo,
            
        ));
    }
}

fn write_applets(html: &mut String, applets: Applets) {
    html.push_str(r#"<div class="col-span-full mb-2">
                <h2 class="text-3xl font-bold text-zinc-800 dark:text-white mb-4">
                    Applets
                </h2>
            </div>
            "#,
    );
    for project in applets.list {
        html.push_str(&format!(
            r#"<div class="bg-white/50 dark:bg-neutral-800 rounded-xl shadow-lg p-6 flex flex-col items-start hover:scale-105 transition-transform">
                {}
                <h2 class="text-2xl font-bold text-zinc-800 dark:text-white mb-2">{}</h2>
                <p class="text-zinc-800 dark:text-white mb-4">{}</p>
                <a href="{}" class="mt-auto text-zinc-800 dark:text-white font-semibold hover:underline">View Project →</a>
            </div>
            "#,
            match project.image {
                Some(img) => format!(r#"<a href="{}" class="w-full h-50 object-cover rounded-lg mb-4">
                    <img src="{}" alt="{}" class="w-full h-50 object-cover rounded-lg mb-4"/>
                </a>"#, img, img, project.name),
                None => "".to_string(),
            },
            project.name,
            project.description,
            project.repo,
            
        ));
    }
}

fn write_themes(html: &mut String, themes: Themes) {
    html.push_str(r#"<div class="col-span-full mb-2">
                <h2 class="text-3xl font-bold text-zinc-800 dark:text-white mb-4">
                    Themes
                </h2>
            </div>
            "#,
    );
    for project in themes.list {
        html.push_str(&format!(
            r#"<div class="bg-white/50 dark:bg-neutral-800 rounded-xl shadow-lg p-6 flex flex-col items-start hover:scale-105 transition-transform">
                {}
                <h2 class="text-2xl font-bold text-zinc-800 dark:text-white mb-2">{}</h2>
                <p class="text-zinc-800 dark:text-white mb-4">{}</p>
                <a href="{}" class="mt-auto text-zinc-800 dark:text-white font-semibold hover:underline">View Project →</a>
            </div>
            "#,
            match project.image {
                Some(img) => format!(r#"<a href="{}" class="w-full h-50 object-cover rounded-lg mb-4">
                    <img src="{}" alt="{}" class="w-full h-50 object-cover rounded-lg mb-4"/>
                </a>"#, img, img, project.name),
                None => "".to_string(),
            },
            project.name,
            project.description,
            project.repo,
            
        ));
    }
}

fn write_services(html: &mut String, services: Services) {
    html.push_str(r#"<div class="col-span-full mb-2">
                <h2 class="text-3xl font-bold text-zinc-800 dark:text-white mb-4">
                    Services
                </h2>
            </div>
            "#,
    );
    for project in services.list {
        html.push_str(&format!(
            r#"<div class="bg-white/50 dark:bg-neutral-800 rounded-xl shadow-lg p-6 flex flex-col items-start hover:scale-105 transition-transform">
                {}
                <h2 class="text-2xl font-bold text-zinc-800 dark:text-white mb-2">{}</h2>
                <p class="text-zinc-800 dark:text-white mb-4">{}</p>
                <a href="{}" class="mt-auto text-zinc-800 dark:text-white font-semibold hover:underline">View Project →</a>
            </div>
            "#,
            match project.image {
                Some(img) => format!(r#"<a href="{}" class="w-full h-50 object-cover rounded-lg mb-4">
                    <img src="{}" alt="{}" />
                </a>"#, img, img, project.name),
                None => "".to_string(),
            },
            project.name,
            project.description,
            project.repo,
            
        ));
    }
}

fn write_scripts(html: &mut String, scripts: Scripts) {
    html.push_str(r#"<div class="col-span-full mb-2">
                <h2 class="text-3xl font-bold text-zinc-800 dark:text-white mb-4">
                    Scripts
                </h2>
            </div>
            "#,
    );
    for project in scripts.list {
        html.push_str(&format!(
            r#"<div class="bg-white/50 dark:bg-neutral-800 rounded-xl shadow-lg p-6 flex flex-col items-start hover:scale-105 transition-transform">
                {}
                <h2 class="text-2xl font-bold text-zinc-800 dark:text-white mb-2">{}</h2>
                <p class="text-zinc-800 dark:text-white mb-4">{}</p>
                <a href="{}" class="mt-auto text-zinc-800 dark:text-white font-semibold hover:underline">View Project →</a>
            </div>
            "#,
            match project.image {
                Some(img) => format!(r#"<a href="{}" class="w-full h-50 object-cover rounded-lg mb-4">
                    <img src="{}" alt="{}" class="w-full h-50 object-cover rounded-lg mb-4"/>
                </a>"#, img, img, project.name),
                None => "".to_string(),
            },
            project.name,
            project.description,
            project.repo,
            
        ));
    }
}
