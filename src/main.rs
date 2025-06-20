mod html;
mod readme;
mod types;

fn main() -> std::io::Result<()> {
    readme::generate()?;
    html::generate()?;
    Ok(())
}
