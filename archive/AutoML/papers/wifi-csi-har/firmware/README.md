# ESP32-C6 CSI Firmware Setup

## Hardware Requirements

| Component | Model | Purpose |
|-----------|-------|---------|
| MCU | **ESP32-C6** | WiFi 6 CSI receiver |
| Router | **Xiaomi BE3600** | WiFi AP / CSI transmitter |
| Cable | USB-C to Mac | Power + serial data |

## Firmware: esp-csi `csi_recv_router`

### 1. Install ESP-IDF

```bash
# macOS
brew install cmake ninja dfu-util
mkdir -p ~/esp
cd ~/esp
git clone --recursive https://github.com/espressif/esp-idf.git -b v5.3
cd esp-idf
./install.sh esp32c6
source export.sh
```

### 2. Clone esp-csi

```bash
cd ~/esp
git clone https://github.com/espressif/esp-csi.git
cd esp-csi/examples/get_csi/csi_recv_router
```

### 3. Configure

```bash
idf.py set-target esp32c6
idf.py menuconfig

# Navigate to:
# Component config → Wi-Fi →
#   - Set SSID: your_xiaomi_ssid
#   - Set Password: your_wifi_password
#   - Channel: 1 (or auto)
#
# CSI Config →
#   - Enable CSI: yes
#   - CSI output format: RAW
```

### 4. Build & Flash

```bash
idf.py build
idf.py -p /dev/tty.usbmodem* flash monitor
```

### 5. Verify CSI Output

You should see output like:
```
CSI_DATA,aa:bb:cc:dd:ee:ff,-45,11,1,0,0,0,...,12,-3,8,5,-2,7,...
```

## Data Collection

```bash
# Single activity
python experiments/data/collect_esp32.py \
    --port /dev/tty.usbmodem* \
    --activity walk \
    --subject 1 \
    --duration 5

# Batch mode (all activities)
python experiments/data/collect_esp32.py \
    --port /dev/tty.usbmodem* \
    --batch \
    --subject 1 \
    --repeats 20 \
    --duration 5
```

## Router Configuration (Xiaomi BE3600)

For best CSI quality:
1. Set to **2.4GHz band** (better penetration)
2. Channel bandwidth: **HT20** (most stable CSI)
3. Fixed channel: **Channel 1** or **6** (avoid DFS)
4. Disable band steering
5. Place router and ESP32-C6 **3-5 meters apart**, line of sight
