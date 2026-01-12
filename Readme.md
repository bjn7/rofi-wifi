# Rofi wifi

A WiFi network manager plugin for Rofi.

## Installation

#### Via AUR Package (using yay helper):

```bash
yay -S rofi-wifi
```

#### Manually:

```bash
command -v cargo &>/dev/null || { echo "Rust (cargo) not found"; exit 1; } && command -v clang &>/dev/null || { echo "Clang compiler not found"; exit 1; } && command -v rofi &>/dev/null || { echo "Rofi not found"; exit 1; } && git clone https://github.com/bjn7/rofi-wifi && cd rofi-wifi && cargo build --release --locked && sudo mv target/release/libwifi.so /usr/lib/rofi
```

### Usage

```bash
rofi -show wifi -iface wlo1
```

Where: `iface` is the required network interface name. To view your Wi-Fi interface name, use the `iwconfig` command.

### Actions

| Default key in Rofi                | Action                                                           |
| ---------------------------------- | ---------------------------------------------------------------- |
| <kbd>Esc</kbd>                     | Exits, or if in password mode, goes back to the Wi-Fi list.      |
| <kbd>Enter</kbd>                   | Connects to a Wi-Fi network, prompts for a password if required. |
| <kbd>Shift</kbd>+<kbd>Delete</kbd> | Forgets the Wi-Fi network.                                       |

### Externally connected wifi

Once you provide the password, you won't be prompted for it again unless the connection is forgotten. However, it will ask for the password if you have previously connected from an external source, e.g. nmcli.

To delete a saved Wi-Fi network, follow these steps if you already have it saved from an external source:

1. To view the list of saved Wi-Fi networks:
   `nmcli connection`

2. To delete a saved network before connecting via a plugin:
   `nmcli connection delete "Your Wi-Fi Name"`

3. Then, use rofi to connect to a Wi-Fi network:
   `rofi -show wifi -iface wlo1`

Select the Wi-Fi network, then connect.

### Connect to a Hidden Wi-Fi Network

Enter the network name in the Wi-Fi search/filter. After typing the Wi-Fi name, press Enter, and you will be prompted to enter the password.

## Configuration

Example: `config.rasi`

```css
configuration {
  wifi {
    modi: "drun,run,window,wifi";
    state-connecting-indicator: [ "connecting .", "connecting ..", "connecting ..."];
    state-connecting-fps: 4;

    state-scan-indicator: [ "⠻", "⠽", "⠾", "⠷", "⠯", "⠟"];
    state-scan-fps: 10;

    // Five icons must be provided, otherwise the default will be used.

    icon-open: [ "󰤨", "󰤥", "󰤢", "󰤟", "󰤯"];

    // Five icons must be provided, otherwise the default will be used.
    icon-psk: [ "󰤪", "󰤧", "󰤤", "󰤡", "󰤬"];
  }
}
```

`state-connecting-indicator`: The frames for the animation used to indicate a connection attempt.

`state-connecting-fps`: Refers to the frame delay for the connecting animation, not the standard FPS abbreviation. A higher value does not necessarily mean better animation. It simply controls how quickly each frame is displayed, and the value should be between 1 and 60. If the value is outside this range, the default value will be used.

`state-scan-indicator`: The frames for the animation used to indicate a scanning process.

`state-scan-fps`: Refers to the frame delay for the scanning animation frames.

`icon-open`: Icons to be displayed for open Wi-Fi networks. Exactly 5 icons must be provided; otherwise, the default icons will be used.

`icon-close`: Icons to be displayed for protected Wi-Fi networks. Exactly 5 icons must be provided; otherwise, the default icons will be used.
