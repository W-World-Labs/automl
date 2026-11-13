from PIL import Image

def make_transparent(img_path, output_path, fuzz_percent=6):
    img = Image.open(img_path).convert("RGBA")
    datas = img.getdata()
    
    new_data = []
    # Any pixel close to white
    threshold = 255 * (1 - fuzz_percent / 100)
    
    for item in datas:
        # Check if the pixel is close to white (R, G, B > threshold)
        if item[0] > threshold and item[1] > threshold and item[2] > threshold:
            # Change the white (also close to white) pixels to transparent
            new_data.append((255, 255, 255, 0))
        else:
            new_data.append(item)
            
    img.putdata(new_data)
    img.save(output_path, "PNG")

make_transparent("source-logo.jpeg", "logo-auto-transparent.png")
print("Done")
