use std::cell::RefCell;

use adw::traits::BinExt;
use gettextrs::gettext;
use gtk::gdk;
use gtk::glib;
use gtk::glib::clone;
use gtk::glib::closure;
use gtk::glib::WeakRef;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::CompositeTemplate;
use once_cell::sync::Lazy;

use crate::model;
use crate::utils;
use crate::view;

const ACTION_INSPECT: &str = "container-details-page.inspect";
const ACTION_SHOW_LOG: &str = "container-details-page.show-log";
const ACTION_SHOW_PROCESSES: &str = "container-details-page.show-processes";

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/com/github/marhkb/Pods/ui/container/details-page.ui")]
    pub(crate) struct DetailsPage {
        pub(super) container: WeakRef<model::Container>,
        pub(super) handler_id: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub(super) back_navigation_controls: TemplateChild<view::BackNavigationControls>,
        #[template_child]
        pub(super) menu_button: TemplateChild<view::ContainerMenuButton>,
        #[template_child]
        pub(super) resources_quick_reference_group:
            TemplateChild<view::ContainerResourcesQuickReferenceGroup>,
        #[template_child]
        pub(super) leaflet_overlay: TemplateChild<view::LeafletOverlay>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DetailsPage {
        const NAME: &'static str = "PdsContainerDetailsPage";
        type Type = super::DetailsPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action(ACTION_INSPECT, None, move |widget, _, _| {
                widget.show_inspection();
            });

            klass.install_action(ACTION_SHOW_LOG, None, move |widget, _, _| {
                widget.show_log();
            });
            klass.install_action(ACTION_SHOW_PROCESSES, None, move |widget, _, _| {
                widget.show_processes();
            });

            add_binding_action(
                klass,
                gdk::Key::F10,
                gdk::ModifierType::SHIFT_MASK,
                "container.start",
            );

            add_binding_action(
                klass,
                gdk::Key::F2,
                gdk::ModifierType::CONTROL_MASK,
                "container.stop",
            );

            add_binding_action(
                klass,
                gdk::Key::F5,
                gdk::ModifierType::CONTROL_MASK,
                "container.restart",
            );

            add_binding_action(
                klass,
                gdk::Key::F6,
                gdk::ModifierType::SHIFT_MASK,
                "container.rename",
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for DetailsPage {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpecObject::new(
                    "container",
                    "Container",
                    "The container of this details page",
                    model::Container::static_type(),
                    glib::ParamFlags::READWRITE
                        | glib::ParamFlags::CONSTRUCT
                        | glib::ParamFlags::EXPLICIT_NOTIFY,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
            match pspec.name() {
                "container" => obj.set_container(value.get().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "container" => obj.container().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            Self::Type::this_expression("container")
                .chain_property::<model::Container>("status")
                .chain_closure::<bool>(closure!(
                    |_: glib::Object, status: model::ContainerStatus| status
                        == model::ContainerStatus::Running
                ))
                .bind(&*self.resources_quick_reference_group, "visible", Some(obj));
        }

        fn dispose(&self, obj: &Self::Type) {
            utils::ChildIter::from(obj).for_each(|child| child.unparent());
        }
    }

    impl WidgetImpl for DetailsPage {}
}

glib::wrapper! {
    pub(crate) struct DetailsPage(ObjectSubclass<imp::DetailsPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl From<&model::Container> for DetailsPage {
    fn from(image: &model::Container) -> Self {
        glib::Object::new(&[("container", image)])
            .expect("Failed to create PdsContainerDetailsPage")
    }
}

impl DetailsPage {
    fn container(&self) -> Option<model::Container> {
        self.imp().container.upgrade()
    }

    fn set_container(&self, value: Option<&model::Container>) {
        if self.container().as_ref() == value {
            return;
        }

        let imp = self.imp();

        if let Some(container) = self.container() {
            container.disconnect(imp.handler_id.take().unwrap());
        }

        if let Some(container) = value {
            container.inspect(clone!(@weak self as obj => move |e| {
                utils::show_error_toast(&obj, &gettext("Error on loading container details"), &e.to_string());
            }));

            let handler_id = container.connect_deleted(clone!(@weak self as obj => move |container| {
                utils::show_toast(&obj, &gettext!("Container '{}' has been deleted", container.name()));
                obj.imp().back_navigation_controls.navigate_back();
            }));
            imp.handler_id.replace(Some(handler_id));
        }

        imp.container.set(value);
        self.notify("container");
    }

    fn show_inspection(&self) {
        if let Some(container) = self
            .container()
            .as_ref()
            .and_then(model::Container::api_container)
        {
            self.imp()
                .leaflet_overlay
                .show_details(&view::InspectionPage::from(view::Inspectable::Container(
                    container,
                )));
        }
    }

    fn show_log(&self) {
        if let Some(container) = self.container() {
            self.imp()
                .leaflet_overlay
                .show_details(&view::ContainerLogPage::from(&container));
        }
    }

    fn show_processes(&self) {
        if let Some(container) = self.container() {
            self.imp()
                .leaflet_overlay
                .show_details(&view::TopPage::from(&container));
        }
    }
}

fn add_binding_action(
    klass: &mut <imp::DetailsPage as ObjectSubclass>::Class,
    keyval: gdk::Key,
    mods: gdk::ModifierType,
    action: &'static str,
) {
    klass.add_binding(
        keyval,
        mods,
        |widget, _| {
            let imp = widget.imp();
            match imp.leaflet_overlay.child() {
                None => imp.menu_button.activate_action(action, None).is_ok(),
                Some(_) => false,
            }
        },
        None,
    );

    // For displaying a mnemonic.
    klass.add_binding_action(keyval, mods, action, None);
}
