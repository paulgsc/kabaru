use octocrab::{Octocrab, OctocrabBuilder};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use std::env;
use std::fs;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let github_token = env::var("GITHUB_TOKEN")?;
    let repo_name = env::var("GITHUB_REPOSITORY")?;
    let issue_number: u64 = env::var("GITHUB_EVENT_PATH")?.parse()?;

    let octocrab = OctocrabBuilder::new()
        .personal_token(github_token)
        .build()?;

    let repo = octocrab.repos(repo_name.split('/').next().unwrap(), repo_name.split('/').nth(1).unwrap());
    let issue = repo.issues().get(issue_number).await?;

    let paper_url = Regex::new(r"Paper URL: (.+)")?
        .captures(&issue.body.unwrap())
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str())
        .ok_or("Could not find paper URL")?;

    let response = Client::new().get(paper_url).send()?;
    let document = Html::parse_document(&response.text()?);

    let title = document.select(&Selector::parse("h1.title").unwrap()).next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_else(|| "Unknown Title".to_string());

    let authors: Vec<String> = document.select(&Selector::parse("a.author").unwrap())
        .map(|el| el.text().collect())
        .collect();

    let year = document.select(&Selector::parse("span.year").unwrap()).next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_else(|| "Unknown Year".to_string());

    let dir_name = title.replace(|c: char| !c.is_alphanumeric() && c != ' ', "_");
    fs::create_dir_all(Path::new("papers").join(&dir_name))?;

    let meta_content = format!(
        "# Paper Meta Information\n\n## Basic Information\n- **Title**: {}\n- **Authors**: {}\n- **Year**: {}\n- **URL**: {}",
        title,
        authors.join(", "),
        year,
        paper_url
    );

    fs::write(Path::new("papers").join(&dir_name).join("paper_meta.md"), meta_content)?;
    fs::write(Path::new("papers").join(&dir_name).join("summary.md"), "")?;
    fs::write(Path::new("papers").join(&dir_name).join("paper_completion_form.md"), "")?;

    let branch_name = format!("paper/{}", dir_name);
    let main_branch = repo.branches().get("main").await?;
    repo.git().create_ref(&format!("refs/heads/{}", branch_name), main_branch.commit.sha).await?;

    for file in &["paper_meta.md", "summary.md", "paper_completion_form.md"] {
        let path = format!("papers/{}/{}", dir_name, file);
        let content = fs::read_to_string(Path::new("papers").join(&dir_name).join(file))?;
        repo.create_file(&path, &format!("Add {} for {}", file, title), content)
            .branch(&branch_name)
            .send()
            .await?;
    }

    let pr = repo.pulls().create(&format!("[READING] {}", title), &branch_name, "main")
        .body(&format!("Reading in progress for {}", title))
        .send()
        .await?;

    repo.issues().close(issue_number).await?;

    pr.create_comment(&format!("Paper reading setup completed for '{}'. You can now start your reading process!", title)).await?;

    Ok(())
}
