use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use gettextrs::gettext;
use gtk::glib;
use gtk::glib::clone;
use gtk::glib::closure;
use gtk::glib::WeakRef;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::CompositeTemplate;
use once_cell::sync::Lazy;
use once_cell::unsync::OnceCell;

use crate::model;
use crate::podman;
use crate::utils;

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/com/github/marhkb/Pods/ui/component/top-page.ui")]
    pub(crate) struct TopPage {
        /// A `Container` or a `Pod`
        pub(super) top_source: WeakRef<glib::Object>,
        pub(super) tree_store: OnceCell<gtk::TreeStore>,
        #[template_child]
        pub(super) window_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub(super) tree_view: TemplateChild<gtk::TreeView>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TopPage {
        const NAME: &'static str = "PdsTopPage";
        type Type = super::TopPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TopPage {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpecObject::new(
                    "top-source",
                    "Top Source",
                    "The source of the processes of this top page",
                    glib::Object::static_type(),
                    glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            _obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
            match pspec.name() {
                "top-source" => self.top_source.set(value.get().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "top-source" => obj.top_source().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            Self::Type::this_expression("top-source")
                .chain_closure::<String>(closure!(
                    |_: Self::Type, top_source: Option<glib::Object>| {
                        top_source
                            .and_then(|top_source| {
                                if top_source.downcast_ref::<model::Container>().is_some() {
                                    Some(gettext("Container Processes"))
                                } else if top_source.downcast_ref::<model::Pod>().is_some() {
                                    Some(gettext("Pod Processes"))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default()
                    }
                ))
                .bind(&*self.window_title, "title", Some(obj));

            obj.connect_top_stream();
        }

        fn dispose(&self, obj: &Self::Type) {
            utils::ChildIter::from(obj).for_each(|child| child.unparent());
        }
    }

    impl WidgetImpl for TopPage {}
}

glib::wrapper! {
    pub(crate) struct TopPage(ObjectSubclass<imp::TopPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl From<&model::Container> for TopPage {
    fn from(container: &model::Container) -> Self {
        glib::Object::new(&[("top-source", container)]).expect("Failed to create TopPage")
    }
}

impl From<&model::Pod> for TopPage {
    fn from(pod: &model::Pod) -> Self {
        glib::Object::new(&[("top-source", pod)]).expect("Failed to create TopPage")
    }
}

impl TopPage {
    fn top_source(&self) -> Option<glib::Object> {
        self.imp().top_source.upgrade()
    }

    fn connect_top_stream(&self) {
        if let Some(processes_source) = self.top_source().as_ref().and_then(|obj| {
            if let Some(container) = obj.downcast_ref::<model::Container>() {
                container.api().map(|c| Box::new(c) as Box<dyn TopSource>)
            } else if let Some(pod) = obj.downcast_ref::<model::Pod>() {
                pod.api().map(|p| Box::new(p) as Box<dyn TopSource>)
            } else {
                unreachable!("unknown type for top source: {obj:?}")
            }
        }) {
            utils::run_stream(
                processes_source,
                move |container| container.stream(),
                clone!(@weak self as obj => @default-return glib::Continue(false), move |result: podman::Result<TopStreamElement>| {

                    glib::Continue(match result {
                        Ok(top) => {
                            let imp = obj.imp();
                            let tree_store = imp.tree_store.get_or_init(|| {
                                let tree_store = gtk::TreeStore::new(
                                    &top.titles
                                        .iter()
                                        .map(|_| String::static_type())
                                        .collect::<Vec<_>>(),
                                );
                                imp.tree_view.set_model(Some(&tree_store));

                                top.titles.iter().enumerate().for_each(|(i, title)| {
                                    let column = gtk::TreeViewColumn::with_attributes(
                                        title,
                                        &gtk::CellRendererText::new(),
                                        &[("text", i as i32)],
                                    );
                                    column.set_sort_column_id(i as i32);
                                    column.set_sizing(gtk::TreeViewColumnSizing::GrowOnly);
                                    imp.tree_view.append_column(&column);
                                });

                                tree_store
                            });

                            // Remove processes that have disappeared.
                            tree_store.foreach(|_, _, iter| {
                                if !top
                                    .processes
                                    .iter()
                                    .any(|process| process[1] == tree_store.get::<String>(iter, 1))
                                {
                                    tree_store.remove(iter);
                                }
                                false
                            });

                            // Replace and add processes.
                            top.processes.iter().for_each(|process| {
                                let row = process.iter()
                                    .enumerate()
                                    .map(|(i, v)| (i as u32, v as &dyn gtk::prelude::ToValue))
                                    .collect::<Vec<_>>();

                                let mut replaced = false;

                                tree_store.foreach(|_, _, iter| {
                                    if process[1] == tree_store.get::<String>(iter, 1) {
                                        tree_store.set(iter, row.as_slice());
                                        replaced = true;
                                        true
                                    } else {
                                        false
                                    }
                                });

                                if !replaced {
                                    tree_store.set(&tree_store.append(None), row.as_slice());
                                }
                            });

                            true
                        }
                        Err(e) => {
                            log::warn!("Stopping top stream due to error: {e}");
                            false
                        }
                    })
                }),
            );
        }
    }
}

trait TopSource: Send {
    fn stream(&self) -> stream::BoxStream<podman::Result<TopStreamElement>>;
}

impl TopSource for podman::api::Container {
    fn stream(&self) -> stream::BoxStream<podman::Result<TopStreamElement>> {
        self.top_stream(&podman::opts::ContainerTopOpts::builder().delay(2).build())
            .map_ok(|x| TopStreamElement {
                processes: x.processes,
                titles: x.titles,
            })
            .boxed()
    }
}

impl TopSource for podman::api::Pod {
    fn stream(&self) -> stream::BoxStream<podman::Result<TopStreamElement>> {
        self.top_stream(&podman::opts::PodTopOpts::builder().delay(2).build())
            .map_ok(|x| TopStreamElement {
                processes: x.processes,
                titles: x.titles,
            })
            .boxed()
    }
}

struct TopStreamElement {
    pub processes: Vec<Vec<String>>,
    pub titles: Vec<String>,
}
