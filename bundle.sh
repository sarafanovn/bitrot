#!/bin/sh
# Builds a release binary and wraps it into Bitrot.app.
set -e
cd "$(dirname "$0")"
cargo build --release

APP=target/Bitrot.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/bitrot "$APP/Contents/MacOS/Bitrot"

# App icon: assets/icon.png -> AppIcon.icns (regenerate the PNG with assets/make_icon.py).
ICONSET=target/AppIcon.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Bitrot</string>
    <key>CFBundleDisplayName</key><string>Bitrot</string>
    <key>CFBundleIdentifier</key><string>io.github.niksanhts.bitrot</string>
    <key>CFBundleExecutable</key><string>Bitrot</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.2.0</string>
    <key>CFBundleVersion</key><string>2</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Make Finder/Dock pick up the new icon.
touch "$APP"
echo "Built $APP"
echo "Install: cp -R $APP /Applications/"
