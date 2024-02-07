use crate::error::{ScaffoldError, ScaffoldResult};
use crate::file_tree::{dir_content, file_content, load_directory_into_memory, FileTree};
use crate::scaffold::app::cargo::exec_metadata;
use crate::scaffold::app::nix::setup_nix_developer_environment;
use crate::scaffold::app::AppFileTree;
use crate::scaffold::collection::{scaffold_collection, CollectionType};
use crate::scaffold::dna::{scaffold_dna, DnaFileTree};
use crate::scaffold::entry_type::crud::{parse_crud, Crud};
use crate::scaffold::entry_type::definitions::{
    parse_entry_type_reference, parse_referenceable, Cardinality, EntryTypeReference,
    FieldDefinition, FieldType, Referenceable,
};
use crate::scaffold::entry_type::{fields::parse_fields, scaffold_entry_type};
use crate::scaffold::example::{choose_example, Example};
use crate::scaffold::link_type::scaffold_link_type;
use crate::scaffold::web_app::scaffold_web_app;
use crate::scaffold::web_app::uis::{
    choose_non_vanilla_ui_framework, choose_ui_framework, template_for_ui_framework, UiFramework,
};
use crate::scaffold::zome::utils::select_integrity_zomes;
use crate::scaffold::zome::{
    integrity_zome_name, scaffold_coordinator_zome, scaffold_coordinator_zome_in_path,
    scaffold_integrity_zome, scaffold_integrity_zome_with_path, ZomeFileTree,
};
use crate::templates::example::scaffold_example;
use crate::templates::get::get_template;
use crate::templates::{
    choose_or_get_template, choose_or_get_template_file_tree, templates_path, ScaffoldedTemplate,
};
use crate::utils::{
    check_case, check_no_whitespace, input_no_whitespace, input_with_case, input_yes_or_no,
};

use build_fs_tree::{dir, Build, MergeableFileSystemTree};
use convert_case::Case;
use dialoguer::Input;
use dialoguer::{theme::ColorfulTheme, Select};
use std::fs;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{ffi::OsString, path::PathBuf};
use structopt::StructOpt;

/// The list of subcommands for `hc scaffold`
#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::InferSubcommands)]
pub enum HcScaffold {
    /// Scaffold a new, empty web app
    WebApp {
        /// Name of the app to scaffold
        name: Option<String>,

        /// Description of the app to scaffold
        description: Option<String>,

        #[structopt(long)]
        /// Whether to setup the holonix development environment for this web-app
        setup_nix: Option<bool>,

        #[structopt(short = "u", long)]
        /// The git repository URL from which to download the template, incompatible with --templates-path
        templates_url: Option<String>,

        #[structopt(short = "p", long, parse(from_os_str))]
        /// The local folder path in which to look for the given template, incompatible with --templates-url
        templates_path: Option<PathBuf>,

        #[structopt(short, long)]
        /// The template to scaffold the web-app from
        /// If "--templates-url" is given, the template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        /// If not, the template must be an option from the built-in templates: "vanilla", "vue", "lit", "svelte"
        template: Option<String>,

        #[structopt(long = "holo", hidden = true)]
        holo_enabled: bool,
    },
    /// Set up the template used in this project
    Template(HcScaffoldTemplate),
    /// Scaffold a DNA into an existing app
    Dna {
        #[structopt(long)]
        /// Name of the app in which you want to scaffold the DNA
        app: Option<String>,

        /// Name of the DNA being scaffolded
        name: Option<String>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold one or multiple zomes into an existing DNA
    Zome {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        /// Name of the zome being scaffolded
        name: Option<String>,

        #[structopt(long, parse(from_os_str))]
        /// Scaffold an integrity zome at the given path
        integrity: Option<PathBuf>,

        #[structopt(long, parse(from_os_str))]
        /// Scaffold a coordinator zome at the given path
        coordinator: Option<PathBuf>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold an entry type and CRUD functions into an existing zome
    EntryType {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the entry definition
        zome: Option<String>,

        /// Name of the entry type being scaffolded
        name: Option<String>,

        #[structopt(long)]
        /// Whether this entry type should be refereced with its "EntryHash" or its "ActionHash"
        /// If referred to by "EntryHash", the entries can't be updated or deleted
        reference_entry_hash: Option<bool>,

        #[structopt(long, parse(try_from_str = parse_crud))]
        /// The Create, "Read", "Update", and "Delete" zome call functions that should be scaffolded for this entry type
        /// If "--reference-entry-hash" is "true", only "Create" and "Read" will be scaffolded
        crud: Option<Crud>,

        #[structopt(long)]
        /// Whether to create a link from the original entry to each update action
        /// Only applies if update is selected in the "crud" argument
        link_from_original_to_each_update: Option<bool>,

        #[structopt(long, value_delimiter = ",", parse(try_from_str = parse_fields))]
        /// The fields that the entry type struct should contain
        /// Grammar: <FIELD_NAME>:<FIELD_TYPE>:<WIDGET>:<LINKED_FROM> , (widget and linked_from are optional)
        /// Eg. "title:String:TextField" , "posts_hashes:Vec\<ActionHash\>::Post"
        fields: Option<Vec<FieldDefinition>>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold a link type and its appropriate zome functions into an existing zome
    LinkType {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the link type
        zome: Option<String>,

        #[structopt(parse(try_from_str = parse_referenceable))]
        /// Entry type (or agent role) used as the base for the links
        from_referenceable: Option<Referenceable>,

        #[structopt(parse(try_from_str = parse_referenceable))]
        /// Entry type (or agent role) used as the target for the links
        to_referenceable: Option<Referenceable>,

        #[structopt(long)]
        /// Whether to create the inverse link, from the "--to-referenceable" entry type to the "--from-referenceable" entry type
        bidirectional: Option<bool>,

        #[structopt(long)]
        /// Whether this link type can be deleted
        delete: Option<bool>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold a collection of entries in an existing zome
    Collection {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the link type
        zome: Option<String>,

        /// Collection type: "global" or "by-author"
        collection_type: Option<CollectionType>,

        /// Collection name, just to differentiate it from other collections
        collection_name: Option<String>,

        #[structopt(parse(try_from_str = parse_entry_type_reference))]
        /// Entry type that is going to be added to the collection
        entry_type: Option<EntryTypeReference>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },

    Example {
        /// Name of the example to scaffold. One of ['hello-world', 'forum'].
        example: Option<Example>,

        #[structopt(short, long)]
        /// The template to scaffold the example for
        /// Must be an option from the built-in templates: "vanilla", "vue", "lit", "svelte"
        template: Option<String>,
    },
}

impl HcScaffold {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            HcScaffold::WebApp {
                name,
                description,
                setup_nix,
                template,
                templates_url,
                templates_path,
                holo_enabled,
            } => {
                let prompt = String::from("App name (no whitespaces):");
                let name: String = match name {
                    Some(n) => {
                        check_no_whitespace(&n, "app name")?;
                        n
                    }
                    None => input_no_whitespace(&prompt)?,
                };

                let current_dir = std::env::current_dir()?;
                let app_folder = current_dir.join(&name);
                if app_folder.as_path().exists() {
                    return Err(ScaffoldError::FolderAlreadyExists(app_folder.clone()))?;
                }

                let (template_name, template_file_tree, scaffold_template) =
                    match (templates_url, templates_path) {
                        (Some(_), Some(_)) => Err(ScaffoldError::InvalidArguments(String::from(
                            "cannot use --templates-path and --templates-url together",
                        )))?,
                        (Some(u), None) => {
                            let (name, file_tree) = get_template(&u, &template)?;
                            (name, file_tree, true)
                        }
                        (None, Some(p)) => {
                            let templates_dir = current_dir.join(p);
                            let templates_file_tree = load_directory_into_memory(&templates_dir)?;
                            let name = choose_or_get_template(
                                &dir! {".templates"=>templates_file_tree},
                                &template,
                            )?;
                            let template_file_tree =
                                load_directory_into_memory(&templates_dir.join(&name))?;
                            (name, template_file_tree, true)
                        }
                        (None, None) => {
                            let ui_framework = match template {
                                Some(t) => UiFramework::from_str(t.as_str())?,
                                None => choose_ui_framework()?,
                            };

                            (
                                format!("{:?}", ui_framework),
                                template_for_ui_framework(&ui_framework)?,
                                false,
                            )
                        }
                    };

                if file_content(&template_file_tree, &PathBuf::from("web-app/README.md.hbs"))
                    .is_err()
                {
                    return Err(ScaffoldError::MalformedTemplate(
                        "Template does not contain a README.md.hbs file in its \"web-app\" directory"
                            .to_string(),
                    ))?;
                }

                let setup_nix = match setup_nix {
                    Some(s) => s,
                    None => {
                        let holonix_prompt = String::from("Do you want to set up the holonix development environment for this project?");
                        input_yes_or_no(&holonix_prompt, Some(true))?
                    }
                };

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_web_app(
                    name.clone(),
                    description,
                    !setup_nix,
                    &template_file_tree,
                    template_name,
                    scaffold_template,
                    holo_enabled,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(dir! {
                    name.clone() => file_tree
                });

                file_tree.build(&".".into())?;

                let mut maybe_nix = "";

                let app_dir = std::env::current_dir()?.join(&name);
                if setup_nix {
                    if let Err(err) = setup_nix_developer_environment(&app_dir) {
                        fs::remove_dir_all(&app_dir)?;

                        return Err(err)?;
                    }

                    maybe_nix = "\n  nix develop";
                }

                setup_git_environment(&app_dir)?;

                println!(
                    r#"
Web hApp "{}" scaffolded!
"#,
                    name
                );

                if let Some(i) = next_instructions {
                    println!("{}", i);
                } else {
                    println!(
                        r#"
Notice that this is an empty skeleton for a Holochain web-app, so:

- It won't compile until you add a DNA to it, and then add a zome to that DNA.
- The UI is empty, you'll need to import the appropriate components to the top level app component.

Set up your development environment with:

  cd {}{}
  npm install

To continue scaffolding your application, add new DNAs to your app with:

  hc scaffold dna

Then, at any point in time you can start your application with:

  npm start
"#,
                        name, maybe_nix
                    );
                }
            }
            HcScaffold::Template(template) => template.run()?,
            HcScaffold::Dna {
                app,
                name,
                template,
            } => {
                let prompt = String::from("DNA name (snake_case):");
                let name: String = match name {
                    Some(n) => {
                        check_case(&n, "dna name", Case::Snake)?;
                        n
                    }
                    None => input_with_case(&prompt, Case::Snake)?,
                };

                let current_dir = std::env::current_dir()?;

                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let app_file_tree = AppFileTree::get_or_choose(file_tree, &app)?;

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_dna(app_file_tree, &template_file_tree, &name)?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
DNA "{}" scaffolded!"#,
                    name
                );

                if let Some(i) = next_instructions {
                    println!("{}", i);
                } else {
                    println!(
                        r#"
Add new zomes to your DNA with:

  hc scaffold zome
"#,
                    );
                }
            }
            HcScaffold::Zome {
                dna,
                name,
                integrity,
                coordinator,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                if let Some(n) = name.clone() {
                    check_case(&n, "zome name", Case::Snake)?;
                }

                let (scaffold_integrity, scaffold_coordinator) =
                    match (integrity.clone(), coordinator.clone()) {
                        (None, None) => {
                            let option = Select::with_theme(&ColorfulTheme::default())
                                .with_prompt("What do you want to scaffold?")
                                .default(0)
                                .items(&[
                                    "Integrity/coordinator zome-pair (recommended)",
                                    "Only an integrity zome",
                                    "Only a coordinator zome",
                                ])
                                .interact()?;

                            match option {
                                0 => (true, true),
                                1 => (true, false),
                                _ => (false, true),
                            }
                        }
                        _ => (integrity.is_some(), coordinator.is_some()),
                    };

                let name_prompt = match (scaffold_integrity, scaffold_coordinator) {
                    (true, true) => String::from("Enter coordinator zome name (snake_case):\n (The integrity zome will automatically be named '{name of coordinator zome}_integrity')\n"),
                    _ => String::from("Enter zome name (snake_case):"),
                };

                let name: String = match name {
                    Some(n) => n,
                    None => input_with_case(&name_prompt, Case::Snake)?,
                };

                let mut dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;
                let dna_manifest_path = dna_file_tree.dna_manifest_path.clone();

                let mut zome_next_instructions: (Option<String>, Option<String>) = (None, None);

                if scaffold_integrity {
                    let integrity_zome_name = match scaffold_coordinator {
                        true => integrity_zome_name(&name),
                        false => name.clone(),
                    };
                    let ScaffoldedTemplate {
                        file_tree,
                        next_instructions,
                    } = scaffold_integrity_zome(
                        dna_file_tree,
                        &template_file_tree,
                        &integrity_zome_name,
                        &integrity,
                    )?;

                    zome_next_instructions.0 = next_instructions;

                    println!(r#"Integrity zome "{}" scaffolded!"#, integrity_zome_name);

                    dna_file_tree =
                        DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;
                }

                if scaffold_coordinator {
                    let dependencies = match scaffold_integrity {
                        true => Some(vec![integrity_zome_name(&name)]),
                        false => {
                            let integrity_zomes = select_integrity_zomes(&dna_file_tree.dna_manifest, Some(&String::from(
                              "Select integrity zome(s) this coordinator zome depends on (SPACE to select/unselect, ENTER to continue):"
                            )))?;
                            Some(integrity_zomes)
                        }
                    };
                    let ScaffoldedTemplate {
                        file_tree,
                        next_instructions,
                    } = scaffold_coordinator_zome(
                        dna_file_tree,
                        &template_file_tree,
                        &name,
                        &dependencies,
                        &coordinator,
                    )?;
                    zome_next_instructions.1 = next_instructions;

                    println!(r#"Coordinator zome "{}" scaffolded!"#, name);

                    dna_file_tree =
                        DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;
                }

                // TODO: implement scaffold_zome_template

                let file_tree =
                    MergeableFileSystemTree::<OsString, String>::from(dna_file_tree.file_tree());

                let f = file_tree.clone();

                file_tree.build(&".".into())?;

                // Execute cargo metadata to set up the cargo workspace in case this zome is the first crate
                exec_metadata(&f)?;

                match zome_next_instructions {
                    (Some(ii), Some(ci)) => {
                        println!("{}", ii);
                        println!("{}", ci);
                    }
                    (None, Some(i)) => println!("{}", i),
                    (Some(i), None) => println!("{}", i),
                    _ => println!(
                        r#"
Add new entry definitions to your zome with:

  hc scaffold entry-type
"#,
                    ),
                }
            }
            HcScaffold::EntryType {
                dna,
                zome,
                name,
                crud,
                reference_entry_hash,
                link_from_original_to_each_update,
                fields,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let name = match name {
                    Some(n) => {
                        check_case(&n, "entry type name", Case::Snake)?;
                        n
                    }
                    None => input_with_case(
                        &String::from("Entry type name (snake_case):"),
                        Case::Snake,
                    )?,
                };

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_entry_type(
                    zome_file_tree,
                    &template_file_tree,
                    &name,
                    &crud,
                    &reference_entry_hash,
                    &link_from_original_to_each_update,
                    &fields,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Entry type "{}" scaffolded!"#,
                    name
                );

                if let Some(i) = next_instructions {
                    println!("{}", i);
                } else {
                    println!(
                        r#"
Add new collections for that entry type with:

  hc scaffold collection
"#,
                    );
                }
            }
            HcScaffold::LinkType {
                dna,
                zome,
                from_referenceable,
                to_referenceable,
                delete,
                bidirectional,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_link_type(
                    zome_file_tree,
                    &template_file_tree,
                    &from_referenceable,
                    &to_referenceable,
                    &delete,
                    &bidirectional,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Link type scaffolded!
"#,
                );
                if let Some(i) = next_instructions {
                    println!("{}", i);
                }
            }
            HcScaffold::Collection {
                dna,
                zome,
                collection_name,
                collection_type,
                entry_type,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let prompt = String::from("Collection name (snake_case, eg. \"all_posts\"):");
                let name: String = match collection_name {
                    Some(n) => {
                        check_case(&n, "collection name", Case::Snake)?;
                        n
                    }
                    None => input_with_case(&prompt, Case::Snake)?,
                };

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_collection(
                    zome_file_tree,
                    &template_file_tree,
                    &name,
                    &collection_type,
                    &entry_type,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Collection "{}" scaffolded!
"#,
                    name
                );

                if let Some(i) = next_instructions {
                    println!("{}", i);
                }
            }
            HcScaffold::Example { example, template } => {
                let example = match example {
                    Some(e) => e,
                    None => choose_example()?,
                };
                let name = example.to_string();

                let app_dir = std::env::current_dir()?.join(&name);
                if app_dir.as_path().exists() {
                    return Err(ScaffoldError::FolderAlreadyExists(app_dir.clone()))?;
                }

                let ui_framework = match example {
                    Example::HelloWorld => UiFramework::Vanilla,
                    Example::Forum => match template {
                        Some(t) => UiFramework::from_str(t.as_str())?,
                        None => choose_non_vanilla_ui_framework()?,
                    },
                };

                let template_file_tree = template_for_ui_framework(&ui_framework)?;
                let template_name = format!("{:?}", ui_framework);

                // Match on example types
                let file_tree = match example {
                    Example::HelloWorld => {
                        // scaffold web-app
                        let ScaffoldedTemplate { file_tree, .. } = scaffold_web_app(
                            name.clone(),
                            Some(String::from("A simple 'hello world' application.")),
                            false,
                            &template_file_tree,
                            template_name.clone(),
                            false,
                            false,
                        )?;

                        file_tree
                    }
                    Example::Forum => {
                        // scaffold web-app
                        let ScaffoldedTemplate { file_tree, .. } = scaffold_web_app(
                            name.clone(),
                            Some(String::from("A simple 'forum' application.")),
                            false,
                            &template_file_tree,
                            template_name.clone(),
                            false,
                            false,
                        )?;

                        // scaffold dna hello_world
                        let dna_name = String::from("forum");

                        let app_file_tree =
                            AppFileTree::get_or_choose(file_tree, &Some(name.clone()))?;
                        let ScaffoldedTemplate { file_tree, .. } =
                            scaffold_dna(app_file_tree, &template_file_tree, &dna_name)?;

                        // scaffold integrity zome posts
                        let dna_file_tree =
                            DnaFileTree::get_or_choose(file_tree, &Some(dna_name.clone()))?;
                        let dna_manifest_path = dna_file_tree.dna_manifest_path.clone();

                        let integrity_zome_name = String::from("posts_integrity");
                        let integrity_zome_path = PathBuf::new()
                            .join("dnas")
                            .join(&dna_name)
                            .join("zomes")
                            .join("integrity");
                        let ScaffoldedTemplate { file_tree, .. } =
                            scaffold_integrity_zome_with_path(
                                dna_file_tree,
                                &template_file_tree,
                                &integrity_zome_name,
                                &integrity_zome_path,
                            )?;

                        let dna_file_tree =
                            DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;

                        let coordinator_zome_name = String::from("posts");
                        let coordinator_zome_path = PathBuf::new()
                            .join("dnas")
                            .join(dna_name)
                            .join("zomes")
                            .join("coordinator");
                        let ScaffoldedTemplate { file_tree, .. } =
                            scaffold_coordinator_zome_in_path(
                                dna_file_tree,
                                &template_file_tree,
                                &coordinator_zome_name,
                                &Some(vec![integrity_zome_name.clone()]),
                                &coordinator_zome_path,
                            )?;

                        // Scaffold the app here to enable ZomeFileTree::from_manifest(), which calls `cargo metadata`
                        MergeableFileSystemTree::<OsString, String>::from(file_tree.clone())
                            .build(&app_dir)?;

                        std::env::set_current_dir(&app_dir)?;

                        let dna_file_tree =
                            DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;

                        let zome_file_tree = ZomeFileTree::get_or_choose_integrity(
                            dna_file_tree,
                            &Some(integrity_zome_name.clone()),
                        )?;

                        let ScaffoldedTemplate { file_tree, .. } = scaffold_entry_type(
                            zome_file_tree,
                            &template_file_tree,
                            &String::from("post"),
                            &Some(Crud {
                                update: true,
                                delete: true,
                            }),
                            &Some(false),
                            &Some(true),
                            &Some(vec![
                                FieldDefinition {
                                    field_name: String::from("title"),
                                    field_type: FieldType::String,
                                    widget: Some(String::from("TextField")),
                                    cardinality: Cardinality::Single,
                                    linked_from: None,
                                },
                                FieldDefinition {
                                    field_name: String::from("content"),
                                    field_type: FieldType::String,
                                    widget: Some(String::from("TextArea")),
                                    cardinality: Cardinality::Single,
                                    linked_from: None,
                                },
                            ]),
                        )?;

                        let dna_file_tree =
                            DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;

                        let zome_file_tree = ZomeFileTree::get_or_choose_integrity(
                            dna_file_tree,
                            &Some(String::from("posts_integrity")),
                        )?;

                        let ScaffoldedTemplate { file_tree, .. } = scaffold_entry_type(
                            zome_file_tree,
                            &template_file_tree,
                            &String::from("comment"),
                            &Some(Crud {
                                update: false,
                                delete: true,
                            }),
                            &Some(false),
                            &Some(true),
                            &Some(vec![
                                FieldDefinition {
                                    field_name: String::from("comment"),
                                    field_type: FieldType::String,
                                    widget: Some(String::from("TextArea")),
                                    cardinality: Cardinality::Single,
                                    linked_from: None,
                                },
                                FieldDefinition {
                                    field_name: String::from("post_hash"),
                                    field_type: FieldType::ActionHash,
                                    widget: None,
                                    cardinality: Cardinality::Single,
                                    linked_from: Some(Referenceable::EntryType(
                                        EntryTypeReference {
                                            entry_type: String::from("post"),
                                            reference_entry_hash: false,
                                        },
                                    )),
                                },
                            ]),
                        )?;

                        let dna_file_tree =
                            DnaFileTree::from_dna_manifest_path(file_tree, &dna_manifest_path)?;

                        let zome_file_tree = ZomeFileTree::get_or_choose_integrity(
                            dna_file_tree,
                            &Some(String::from("posts_integrity")),
                        )?;

                        let ScaffoldedTemplate { file_tree, .. } = scaffold_collection(
                            zome_file_tree,
                            &template_file_tree,
                            &String::from("all_posts"),
                            &Some(CollectionType::Global),
                            &Some(EntryTypeReference {
                                entry_type: String::from("post"),
                                reference_entry_hash: false,
                            }),
                        )?;

                        file_tree
                    }
                };

                let ScaffoldedTemplate {
                    file_tree,
                    next_instructions,
                } = scaffold_example(file_tree, &template_file_tree, &example)?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(file_tree);

                file_tree.build(&app_dir)?;

                // set up nix
                if let Err(err) = setup_nix_developer_environment(&app_dir) {
                    fs::remove_dir_all(&app_dir)?;

                    return Err(err)?;
                }

                setup_git_environment(&app_dir)?;

                println!(
                    r#"
Example "{}" scaffolded!
"#,
                    example.to_string()
                );

                if let Some(i) = next_instructions {
                    println!("{}", i);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::InferSubcommands)]
pub enum HcScaffoldTemplate {
    /// Download a custom template from a remote repository to this folder
    Get {
        /// The git repository URL from which to download the template
        template_url: String,

        #[structopt(long)]
        /// The template to get from the given repository (located at ".templates/<FROM TEMPLATE>")
        from_template: Option<String>,

        #[structopt(long)]
        /// The folder to download the template to, will end up at ".templates/<TO TEMPLATE>"
        to_template: Option<String>,
    },
    /// Initialize a new custom template from a built-in one
    Init {
        /// The UI framework to use as the template for this web-app
        template: Option<UiFramework>,

        #[structopt(long)]
        /// The folder to download the template to, will end up at ".templates/<TO TEMPLATE>"
        to_template: Option<String>,
    },
}

fn existing_templates_names(file_tree: &FileTree) -> ScaffoldResult<Vec<String>> {
    let templates_path = PathBuf::new().join(templates_path());

    match dir_content(file_tree, &templates_path) {
        Ok(templates_dir_content) => {
            let templates: Vec<String> = templates_dir_content
                .into_keys()
                .map(|k| k.to_str().unwrap().to_string())
                .collect();
            Ok(templates)
        }
        _ => Ok(vec![]),
    }
}

fn choose_existing_template(file_tree: &FileTree) -> ScaffoldResult<String> {
    let templates = existing_templates_names(file_tree)?;
    match templates.len() {
        0 => Err(ScaffoldError::NoTemplatesFound),
        1 => Ok(templates[0].clone()),
        _ => {
            let option = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Which existing template should the new template be merged into?")
                .default(0)
                .items(&templates[..])
                .interact()?;

            Ok(templates[option].clone())
        }
    }
}

impl HcScaffoldTemplate {
    pub fn run(self) -> anyhow::Result<()> {
        let (template_name, template_file_tree) = self.get_template_file_tree()?;

        let target_template = match self.target_template() {
            Some(t) => t,
            None => {
                let current_dir = std::env::current_dir()?;

                let file_tree = load_directory_into_memory(&current_dir)?;

                let mut create = true;
                // If existing templates
                if existing_templates_names(&file_tree)?.len() != 0 {
                    // Merge or create?

                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Do you want to create a new template in this repository?")
                        .default(0)
                        .item("Merge with an existing template")
                        .item("Create a new template")
                        .interact()?;

                    if selection == 0 {
                        create = false;
                    }
                }

                if create {
                    // Enter template name
                    let template_name = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("Enter new template name:")
                        .with_initial_text(template_name)
                        .interact()?;
                    template_name
                } else {
                    let existing_template = choose_existing_template(&file_tree)?;
                    existing_template
                }
            }
        };

        let template_file_tree = dir! {
            templates_path().join(&target_template) => template_file_tree
        };

        let file_tree = MergeableFileSystemTree::<OsString, String>::from(template_file_tree);

        file_tree.build(&".".into())?;

        match self {
            HcScaffoldTemplate::Get { .. } => {
                println!(
                    r#"Template downloaded to folder {:?}
"#,
                    templates_path().join(target_template)
                );
            }
            HcScaffoldTemplate::Init { .. } => {
                println!(
                    r#"Template initialized to folder {:?}
"#,
                    templates_path().join(target_template)
                );
            }
        }
        Ok(())
    }

    pub fn target_template(&self) -> Option<String> {
        match self {
            HcScaffoldTemplate::Get {
                to_template: target_template,
                ..
            } => target_template.clone(),
            HcScaffoldTemplate::Init {
                to_template: target_template,
                ..
            } => target_template.clone(),
        }
    }

    pub fn get_template_file_tree(&self) -> ScaffoldResult<(String, FileTree)> {
        match self {
            HcScaffoldTemplate::Get {
                template_url,
                from_template: template,
                ..
            } => get_template(template_url, template),

            HcScaffoldTemplate::Init { template, .. } => {
                let ui_framework = match template {
                    Some(t) => t.clone(),
                    None => choose_ui_framework()?,
                };
                Ok((
                    format!("{}", ui_framework.to_string()),
                    template_for_ui_framework(&ui_framework)?,
                ))
            }
        }
    }
}

fn setup_git_environment(path: &PathBuf) -> ScaffoldResult<()> {
    let output = Command::new("git")
        .stdout(Stdio::inherit())
        .current_dir(path)
        .args(["init", "--initial-branch=main"])
        .output()?;

    if !output.status.success() {
        let output = Command::new("git")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .current_dir(path)
            .args(["init"])
            .output()?;
        if !output.status.success() {
            println!("Warning: error running \"git init\"");
            return Ok(());
        }

        let _output = Command::new("git")
            .current_dir(path)
            .args(["branch", "main"])
            .output()?;
    }

    let output = Command::new("git")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(&path)
        .args(["add", "."])
        .output()?;

    if !output.status.success() {
        println!("Warning: error running \"git add .\"");
    }
    Ok(())
}
