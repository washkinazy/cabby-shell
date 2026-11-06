//! Repository-owned outline symbols; no icon font or theme package is required.
use iced::widget::svg;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Symbol {
    #[default]
    Volume,
    Muted,
    Bluetooth,
    BluetoothOff,
    Wired,
    Wifi,
    Offline,
    Moon,
    MoonOff,
    Clipboard,
    Screenshot,
    Apps,
    Lock,
    Warning,
}
impl Symbol {
    pub fn handle(self) -> svg::Handle {
        let path = match self {
            Self::Volume => {
                "<path d='M3 9h4l5-4v14l-5-4H3zM16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14'/>"
            }
            Self::Muted => "<path d='M3 9h4l5-4v14l-5-4H3zM16 9l6 6m0-6-6 6'/>",
            Self::Bluetooth => "<path d='m7 7 10 10-5 5V2l5 5L7 17'/>",
            Self::BluetoothOff => "<path d='m7 7 10 10-5 5V2l5 5-4 4M7 17l3-3M3 3l18 18'/>",
            Self::Wired => {
                "<rect x='5' y='3' width='14' height='11' rx='2'/><path d='M12 14v5M4 21v-2h16v2M8 6v3m4-3v3m4-3v3'/>"
            }
            Self::Wifi => {
                "<path d='M2 8a16 16 0 0 1 20 0M5 12a11 11 0 0 1 14 0M8 16a6 6 0 0 1 8 0'/><circle cx='12' cy='20' r='1' fill='white'/>"
            }
            Self::Offline => {
                "<path d='M3 8a16 16 0 0 1 18 0M6 12a11 11 0 0 1 11-1M9 16a6 6 0 0 1 6 0M3 3l18 18'/>"
            }
            Self::Moon => "<path d='M20 15a9 9 0 0 1-11-11 9 9 0 1 0 11 11Z'/>",
            Self::MoonOff => {
                "<path d='M20 15a9 9 0 0 1-6 0M9 4a9 9 0 0 0 1 7M5 5a9 9 0 0 0 14 14M3 3l18 18'/>"
            }
            Self::Clipboard => {
                "<rect x='5' y='4' width='14' height='18' rx='2'/><rect x='9' y='2' width='6' height='5' rx='1'/><path d='M9 12h6m-6 5h6'/>"
            }
            Self::Screenshot => {
                "<path d='M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5'/><rect x='7' y='7' width='10' height='10' rx='1'/>"
            }
            Self::Apps => {
                "<rect x='3' y='3' width='6' height='6' rx='1'/><rect x='15' y='3' width='6' height='6' rx='1'/><rect x='3' y='15' width='6' height='6' rx='1'/><rect x='15' y='15' width='6' height='6' rx='1'/>"
            }
            Self::Lock => {
                "<rect x='5' y='10' width='14' height='12' rx='2'/><path d='M8 10V6a4 4 0 0 1 8 0v4m-4 5v3'/>"
            }
            Self::Warning => "<path d='m12 3 10 18H2Z M12 9v5m0 3v1'/>",
        };
        svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><g fill='none' stroke='white' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'>{path}</g></svg>").into_bytes())
    }
}
pub(crate) fn view<'a>(handle: svg::Handle) -> svg::Svg<'a, iced::Theme> {
    svg(handle)
        .width(18)
        .height(18)
        .style(|theme: &iced::Theme, _| svg::Style {
            color: Some(theme.palette().text),
        })
}
