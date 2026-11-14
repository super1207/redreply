use super::*;

pub(super) fn register() {
    add_fun(vec!["图片信息", "图像信息"], |self_t, params| {
        let bin = self_t.get_param_bin(params, 0)?;
        let (img_fmt, img) = RedLang::parse_bin_to_img_raw(bin)?;
        let mut mp = BTreeMap::new();
        mp.insert("宽".to_string(), rv_text(img.width().to_string()));
        mp.insert("高".to_string(), rv_text(img.height().to_string()));
        let img_fmt_str = match img_fmt {
            image::ImageFormat::Png => "png",
            image::ImageFormat::Jpeg => "jpg",
            image::ImageFormat::Gif => "gif",
            image::ImageFormat::WebP => "webp",
            image::ImageFormat::Bmp => "bmp",
            image::ImageFormat::Ico => "",
            _ => "",
        };
        if img_fmt_str.is_empty() {
            return Err(RedLang::make_err("不能识别的图片格式"));
        } else {
            mp.insert("格式".to_string(), rv_text(img_fmt_str.to_string()));
        }
        Ok(Some(rv_object(mp)))
    });

    add_fun(vec!["透视变换"], |self_t, params| {
        let (_img_fmt, img) = get_param_image(self_t, params, 0)?;
        let dst_t = get_param_text_array(self_t, params, 1)?;
        let src_rv = self_t.get_param(params, 2)?;
        let img_width_str = img.width().to_string();
        let img_height_str = img.height().to_string();
        let src_t: Vec<String> = if src_rv.as_text() == Some("") {
            vec![
                "0".to_string(),
                "0".to_string(),
                img_width_str.clone(),
                "0".to_string(),
                img_width_str.clone(),
                img_height_str.clone(),
                "0".to_string(),
                img_height_str.clone(),
            ]
        } else {
            src_rv
                .expect_array_value()?
                .into_iter()
                .map(|v| v.expect_text_value())
                .collect::<Result<Vec<String>, _>>()?
        };
        if dst_t.len() != 8 || src_t.len() != 8 {
            return Err(RedLang::make_err("透视变换参数错误1"));
        }
        fn cv(v: &[String]) -> Result<[(f32, f32); 4], Box<dyn std::error::Error>> {
            let v_ret = [
                (v[0].parse::<f32>()?, v[1].parse::<f32>()?),
                (v[2].parse::<f32>()?, v[3].parse::<f32>()?),
                (v[4].parse::<f32>()?, v[5].parse::<f32>()?),
                (v[6].parse::<f32>()?, v[7].parse::<f32>()?),
            ];
            Ok(v_ret)
        }
        let dst = cv(&dst_t)?;
        let src = cv(&src_t)?;
        let p = Projection::from_control_points(src, dst)
            .ok_or("Could not compute projection matrix")?
            .invert();
        let mut img2 = warp_with(
            &img,
            |x, y| p * (x, y),
            Interpolation::Bilinear,
            Rgba([0, 0, 0, 0]),
        );
        fn m_min(v: Vec<f32>) -> f32 {
            if v.is_empty() {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i < m {
                    m = i;
                }
            }
            m
        }
        fn m_max(v: Vec<f32>) -> f32 {
            if v.is_empty() {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i > m {
                    m = i;
                }
            }
            m
        }
        let x_min = m_min(vec![dst[0].0, dst[1].0, dst[2].0, dst[3].0]);
        let x_max = m_max(vec![dst[0].0, dst[1].0, dst[2].0, dst[3].0]);
        let y_min = m_min(vec![dst[0].1, dst[1].1, dst[2].1, dst[3].1]);
        let y_max = m_max(vec![dst[0].1, dst[1].1, dst[2].1, dst[3].1]);
        let img_out = image::imageops::crop(
            &mut img2,
            x_min as u32,
            y_min as u32,
            (x_max - x_min) as u32,
            (y_max - y_min) as u32,
        );
        let mm = img_out.to_image();
        let ret = self_t.build_bin_raw_from_img(&mm)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(vec!["图片叠加", "图像叠加"], |self_t, params| {
        fn img_paste(
            img1: ImageBuffer<Rgba<u8>, Vec<u8>>,
            img2: ImageBuffer<Rgba<u8>, Vec<u8>>,
            x: i64,
            y: i64,
        ) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn std::error::Error>> {
            let w = img1.width();
            let h = img1.height();
            let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
            image::imageops::overlay(&mut img, &img2, x, y);
            image::imageops::overlay(&mut img, &img1, 0, 0);
            Ok(img)
        }
        let (_img_fmt, img_vec_big) = get_param_image(self_t, params, 0)?;
        let (_img_fmt, img_vec_sub) = get_param_image(self_t, params, 1)?;
        let text3 = self_t.get_param_text(params, 2)?;
        let text4 = self_t.get_param_text(params, 3)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        let img_out = img_paste(img_vec_big, img_vec_sub, x, y)?;
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片横拼接", "图片拼接"], |self_t, params| {
        let (_, img1) = get_param_image(self_t, params, 0)?;
        let (_, img2) = get_param_image(self_t, params, 1)?;
        let mut back_img = ImageBuffer::new(
            img1.width() + img2.width(),
            std::cmp::max(img1.height(), img2.height()),
        );
        image::imageops::overlay(&mut back_img, &img1, 0, 0);
        image::imageops::overlay(&mut back_img, &img2, img1.width() as i64, 0);
        let ret = self_t.build_bin_raw_from_img(&back_img)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片竖拼接"], |self_t, params| {
        let (_, img1) = get_param_image(self_t, params, 0)?;
        let (_, img2) = get_param_image(self_t, params, 1)?;
        let mut back_img = ImageBuffer::new(
            std::cmp::max(img1.width(), img2.width()),
            img1.height() + img2.height(),
        );
        image::imageops::overlay(&mut back_img, &img1, 0, 0);
        image::imageops::overlay(&mut back_img, &img2, 0, img1.height() as i64);
        let ret = self_t.build_bin_raw_from_img(&back_img)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片截取", "图像截取"], |self_t, params| {
        let (_, img1) = get_param_image(self_t, params, 0)?;
        let x = self_t.get_param_text(params, 1)?.parse::<u32>()?;
        let y = self_t.get_param_text(params, 2)?.parse::<u32>()?;
        let mut width = self_t.get_param_text(params, 3)?.parse::<u32>()?;
        let mut height = self_t.get_param_text(params, 4)?.parse::<u32>()?;
        if x >= img1.width() || y >= img1.height() {
            return Err("图片截取原点超出图片范围".into());
        }
        // x + width 可能溢出 u32，改为反向比较；上方已保证 x < 宽、y < 高
        if width > img1.width() - x {
            width = img1.width() - x;
        }
        if height > img1.height() - y {
            height = img1.height() - y;
        }
        let img2 = img1.view(x, y, width, height);
        let ret = self_t.build_bin_raw_from_img(&img2.to_image())?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片覆盖", "图像覆盖"], |self_t, params| {
        let (_, mut img_big) = get_param_image(self_t, params, 0)?;
        let (_, img_sub) = get_param_image(self_t, params, 1)?;
        let text3 = self_t.get_param_text(params, 2)?;
        let text4 = self_t.get_param_text(params, 3)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        for i in 0..img_sub.width() {
            for j in 0..img_sub.height() {
                let ii = x + i as i64;
                let jj = y + j as i64;
                if ii >= 0
                    && (ii as u32) < img_big.width()
                    && jj >= 0
                    && (jj as u32) < img_big.height()
                {
                    let pix = img_big
                        .get_pixel_mut_checked(ii as u32, jj as u32)
                        .ok_or("image out of bound")?;
                    let pix_sub = img_sub
                        .get_pixel_checked(i, j)
                        .ok_or("image out of bound")?;
                    pix.0[0] = pix_sub.0[0];
                    pix.0[1] = pix_sub.0[1];
                    pix.0[2] = pix_sub.0[2];
                    pix.0[3] = pix_sub.0[3];
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img_big)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片模糊", "图像模糊"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let sigma = self_t.get_param_text(params, 1)?.parse::<f32>()?;
        if !sigma.is_finite() {
            return Err(RedLang::make_err("图片模糊参数sigma必须是有限数值"));
        }
        let img_out = if sigma <= 0. {
            img
        } else {
            imageproc::filter::gaussian_blur_f32(&img, sigma)
        };
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片锐化", "图像锐化"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let sigma = self_t.get_param_text(params, 1)?.parse::<f32>()?;
        if !sigma.is_finite() || sigma <= 0.0 {
            return Err(RedLang::make_err("图片锐化参数sigma必须是有限正数"));
        }
        let threshold_text = self_t.get_param_text(params, 2)?;
        let threshold = if threshold_text.is_empty() {
            0
        } else {
            threshold_text.parse::<i32>()?
        };
        let img_out = image::imageops::unsharpen(&img, sigma, threshold);
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(
        vec!["图片上叠加", "图像上叠加"],
        |self_t, params| {
            let (_img_fmt, mut img_big) = get_param_image(self_t, params, 0)?;
            let (_img_fmt, img_sub) = get_param_image(self_t, params, 1)?;
            let text3 = self_t.get_param_text(params, 2)?;
            let text4 = self_t.get_param_text(params, 3)?;
            let x = text3.parse::<i64>()?;
            let y = text4.parse::<i64>()?;
            image::imageops::overlay(&mut img_big, &img_sub, x, y);
            let ret = self_t.build_bin_raw_from_img(&img_big)?;
            Ok(Some(rv_bin(ret)))
        },
    );
    add_fun(vec!["GIF合成"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        let text2 = self_t.get_param_text(params, 1)?;
        let delay = text2.parse::<u64>()?;
        let mut frame_vec: Vec<image::Frame> = vec![];
        for it in arr {
            let raw = it.expect_bin_value()?;
            let img_t = image::ImageReader::new(Cursor::new(raw)).with_guessed_format()?;
            let img = img_t.decode()?.to_rgba8();
            let fm = image::Frame::from_parts(
                img,
                0,
                0,
                image::Delay::from_saturating_duration(Duration::from_millis(delay)),
            );
            frame_vec.push(fm);
        }
        let mut v: Vec<u8> = vec![];
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut v);
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
            encoder.encode_frames(frame_vec)?;
        }
        Ok(Some(rv_bin(v)))
    });

    add_fun(vec!["GIF分解"], |self_t, params| {
        let gif_bin = self_t.get_param_bin(params, 0)?;
        let gif = image::codecs::gif::GifDecoder::new(Cursor::new(gif_bin))?;
        let gif_frames = gif.into_frames();
        let mut ret_vec: Vec<Rc<RedValue>> = vec![];
        for frame_rst in gif_frames {
            let frame = frame_rst?;
            let img_buf = frame.into_buffer();
            ret_vec.push(rv_bin(self_t.build_bin_raw_from_img(&img_buf)?));
        }
        Ok(Some(Rc::new(RedValue::Array(ret_vec))))
    });

    add_fun(vec!["WEBP分解"], |self_t, params| {
        let webp_bin = self_t.get_param_bin(params, 0)?;
        let webp = webp::AnimDecoder::new(&webp_bin).decode()?;
        let webp_frames = webp.get_frames(0..webp.len()).ok_or("解析webp失败")?;
        let mut ret_vec: Vec<Rc<RedValue>> = vec![];
        for frame in webp_frames {
            let mut bytes: Vec<u8> = Vec::new();
            match frame.get_layout() {
                webp::PixelLayout::Rgb => {
                    let img_buf: ImageBuffer<image::Rgb<u8>, Vec<_>> = ImageBuffer::from_vec(
                        frame.width(),
                        frame.height(),
                        frame.get_image().to_vec(),
                    )
                    .ok_or("解析webp失败")?;
                    img_buf.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
                    ret_vec.push(rv_bin(bytes));
                }
                webp::PixelLayout::Rgba => {
                    let img_buf: ImageBuffer<image::Rgba<u8>, Vec<_>> = ImageBuffer::from_vec(
                        frame.width(),
                        frame.height(),
                        frame.get_image().to_vec(),
                    )
                    .ok_or("解析webp失败")?;
                    ret_vec.push(rv_bin(self_t.build_bin_raw_from_img(&img_buf)?))
                }
            }
        }
        Ok(Some(Rc::new(RedValue::Array(ret_vec))))
    });

    add_fun(vec!["WEBP合成"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        let text2 = self_t.get_param_text(params, 1)?;
        let delay = text2.parse::<i32>()?;
        let mut frame_vec: Vec<webp::AnimFrame> = vec![];
        let mut img_buf_vec = vec![];
        for it in arr {
            let raw = it.expect_bin_value()?;
            let img_t = image::ImageReader::new(Cursor::new(raw)).with_guessed_format()?;
            let img_buf = img_t.decode()?.to_rgba8();
            img_buf_vec.push(img_buf);
        }
        for it in &img_buf_vec {
            let fm = webp::AnimFrame::from_rgba(it, it.width(), it.height(), delay);
            frame_vec.push(fm);
        }
        let width;
        let height;
        if !frame_vec.is_empty() {
            width = frame_vec[0].width();
            height = frame_vec[0].height();
        } else {
            return Err(RedLang::make_err("0张图片不能进行WEBP合成"));
        }
        let mut config = webp::WebPConfig::new()
            .map_err(|error| RedLang::make_err(&format!("WebP 配置创建失败:{error:?}")))?;
        config.lossless = 1;
        config.quality = 100f32;
        let mut wp = webp::AnimEncoder::new(width, height, &config);
        for frame in frame_vec {
            wp.add_frame(frame);
        }
        let binding = wp.encode();
        let v = binding.as_bytes();
        Ok(Some(rv_bin(v.to_vec())))
    });

    add_fun(vec!["图片变圆", "图像变圆"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let out = make_round_square_image(&img);
        let ret = self_t.build_bin_raw_from_img(&out)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["圆角"], |self_t, params| {
        let (_, mut img) = get_param_image(self_t, params, 0)?;
        let left_top_t = self_t.get_param_text(params, 1)?.parse::<u32>()?;
        let right_top_t = self_t.get_param_text(params, 2)?.parse::<u32>()?;
        let right_bottom_t = self_t.get_param_text(params, 3)?.parse::<u32>()?;
        let left_bottom_t = self_t.get_param_text(params, 4)?.parse::<u32>()?;
        let width = img.width();
        let height = img.height();
        // 全部用 i64/i128 计算，避免 x<r 时 (x-r) 的 u32 下溢(debug 直接 panic)，
        // 半径超过图片尺寸时按整图取圆处理
        let lt = left_top_t as i64;
        let rt = right_top_t as i64;
        let rb = right_bottom_t as i64;
        let lb = left_bottom_t as i64;
        let w = width as i64;
        let h = height as i64;
        let corner_check = |x: u32, y: u32, r: i64, cx: i64, cy: i64| {
            if r <= 0 {
                return false;
            }
            let dx = x as i64 - cx;
            let dy = y as i64 - cy;
            let dist = dx as i128 * dx as i128 + dy as i128 * dy as i128;
            dist > r as i128 * r as i128
        };

        for x in 0..left_top_t {
            for y in 0..left_top_t {
                if corner_check(x, y, lt, 0, 0) {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0; //r
                        pix.0[1] = 0; //g
                        pix.0[2] = 0; //b
                        pix.0[3] = 0; //a
                    }
                }
            }
        }

        for x in width.saturating_sub(right_top_t)..width {
            for y in 0..right_top_t {
                if corner_check(x, y, rt, w - rt, 0) {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0; //r
                        pix.0[1] = 0; //g
                        pix.0[2] = 0; //b
                        pix.0[3] = 0; //a
                    }
                }
            }
        }

        for x in width.saturating_sub(right_bottom_t)..width {
            for y in height.saturating_sub(right_bottom_t)..height {
                if corner_check(x, y, rb, w - rb, h - rb) {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0; //r
                        pix.0[1] = 0; //g
                        pix.0[2] = 0; //b
                        pix.0[3] = 0; //a
                    }
                }
            }
        }

        for x in 0..left_bottom_t {
            for y in height.saturating_sub(left_bottom_t)..height {
                if corner_check(x, y, lb, 0, h - lb) {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0; //r
                        pix.0[1] = 0; //g
                        pix.0[2] = 0; //b
                        pix.0[3] = 0; //a
                    }
                }
            }
        }

        let ret = self_t.build_bin_raw_from_img(&img)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片遮罩", "图像遮罩"], |self_t, params| {
        let (_img_fmt, mut img_big) = get_param_image(self_t, params, 0)?;
        let (_img_fmt, img_sub) = get_param_image(self_t, params, 1)?;
        let text3 = self_t.get_param_text(params, 2)?;
        let text4 = self_t.get_param_text(params, 3)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        for i in 0..img_sub.width() {
            for j in 0..img_sub.height() {
                let ii = x + i as i64;
                let jj = y + j as i64;
                if ii >= 0
                    && (ii as u32) < img_big.width()
                    && jj >= 0
                    && (jj as u32) < img_big.height()
                {
                    let pix = img_big
                        .get_pixel_mut_checked(ii as u32, jj as u32)
                        .ok_or("image out of bound")?;
                    let pix_sub = img_sub
                        .get_pixel_checked(i, j)
                        .ok_or("image out of bound")?;
                    pix.0[3] = 255 - pix_sub.0[3];
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img_big)?;
        Ok(Some(rv_bin(ret)))
    });

    fn cala_gray_val(red: u8, green: u8, blue: u8) -> u8 {
        let red_t = red as f32 * 0.3;
        let green_t = green as f32 * 0.589;
        let blue_t = blue as f32 * 0.11;
        (red_t + green_t + blue_t) as u8
    }

    add_fun(vec!["图片变灰", "图像变灰"], |self_t, params| {
        let (_, mut img) = get_param_image(self_t, params, 0)?;
        let width = img.width();
        let height = img.height();
        for x in 0..width {
            for y in 0..height {
                let pix = img
                    .get_pixel_mut_checked(x, y)
                    .ok_or("image out of bound")?;
                let color = cala_gray_val(pix.0[0], pix.0[1], pix.0[2]);
                pix.0[0] = color;
                pix.0[1] = color;
                pix.0[2] = color;
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片反色", "图像反色"], |self_t, params| {
        let (_, mut img) = get_param_image(self_t, params, 0)?;
        let width = img.width();
        let height = img.height();
        for x in 0..width {
            for y in 0..height {
                let pix = img
                    .get_pixel_mut_checked(x, y)
                    .ok_or("image out of bound")?;
                pix.0[0] = 255 - pix.0[0];
                pix.0[1] = 255 - pix.0[1];
                pix.0[2] = 255 - pix.0[2];
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片线稿", "图像线稿"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let sigma = self_t.get_param_text(params, 1)?.parse::<f32>()?;
        let contrast = self_t
            .get_param_text(params, 2)?
            .parse::<f32>()
            .unwrap_or(1.0);

        if !sigma.is_finite() || sigma <= 0. {
            return Err(RedLang::make_err("图片线稿参数sigma必须是有限正数"));
        }

        if !contrast.is_finite() || contrast < 0.0 {
            return Err(RedLang::make_err("对比度参数必须是有限且大于等于0"));
        }

        let (width, height) = img.dimensions();

        // 转为灰度图和反向灰度图
        let mut gray_image: ImageBuffer<image::Luma<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        let mut inverted_gray_image: ImageBuffer<image::Luma<u8>, Vec<u8>> =
            ImageBuffer::new(width, height);
        for x in 0..width {
            for y in 0..height {
                let pix = img.get_pixel_checked(x, y).ok_or("image out of bound")?;
                let color = cala_gray_val(pix.0[0], pix.0[1], pix.0[2]);
                gray_image.put_pixel(x, y, image::Luma([color]));
                inverted_gray_image.put_pixel(x, y, image::Luma([255 - color]));
            }
        }

        // 高斯模糊
        let blurred_img = imageproc::filter::gaussian_blur_f32(&inverted_gray_image, sigma);

        // 颜色减淡
        let mut pencil_sketch: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::new(width, height);
        for x in 0..width {
            for y in 0..height {
                let image_pix = gray_image
                    .get_pixel_checked(x, y)
                    .ok_or("image out of bound")?;
                let mask_pix = blurred_img
                    .get_pixel_checked(x, y)
                    .ok_or("image out of bound")?;
                if mask_pix.0[0] == 255 {
                    // 防止除以0
                    pencil_sketch.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                } else {
                    let val = image_pix.0[0] as f32 / (255 - mask_pix.0[0]) as f32 * 256.0;
                    let u8_val = if val > 255.0 { 255 } else { val as u8 };
                    pencil_sketch.put_pixel(x, y, image::Rgba([u8_val, u8_val, u8_val, 255]));
                }
            }
        }

        // 对比度增强（针对黑底白线）
        if contrast != 1.0 {
            for x in 0..width {
                for y in 0..height {
                    let pix = pencil_sketch.get_pixel(x, y);
                    let val = pix.0[0] as f32 / 255.0; // 归一化到 [0, 1]

                    // 使用幂函数调整对比度
                    // contrast > 1: 压暗暗部，提亮亮部（增强对比）
                    // contrast < 1: 提亮暗部，压暗亮部（降低对比）
                    let new_val = val.powf(1.0 / contrast);
                    let clamped_val = (new_val * 255.0).clamp(0.0, 255.0) as u8;

                    pencil_sketch.put_pixel(
                        x,
                        y,
                        image::Rgba([clamped_val, clamped_val, clamped_val, 255]),
                    );
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&pencil_sketch)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片线稿2", "图像线稿2"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let kernel_size = self_t.get_param_text(params, 1)?.parse::<u32>()?;
        let contrast = self_t
            .get_param_text(params, 2)?
            .parse::<f32>()
            .unwrap_or(1.0);
        if kernel_size == 0 || kernel_size % 2 == 0 {
            return Err(RedLang::make_err(
                "图片线稿参数kernel_size必须是大于0的奇数",
            ));
        }
        if contrast < 0.0 {
            return Err(RedLang::make_err("对比度参数必须大于等于0"));
        }
        let (width, height) = img.dimensions();
        // 转为灰度图和反向灰度图
        let mut gray_image: ImageBuffer<image::Luma<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        let mut inverted_gray_image: ImageBuffer<image::Luma<u8>, Vec<u8>> =
            ImageBuffer::new(width, height);
        for x in 0..width {
            for y in 0..height {
                let pix = img.get_pixel_checked(x, y).ok_or("image out of bound")?;
                let color = cala_gray_val(pix.0[0], pix.0[1], pix.0[2]);
                gray_image.put_pixel(x, y, image::Luma([color]));
                inverted_gray_image.put_pixel(x, y, image::Luma([255 - color]));
            }
        }
        // 最小值滤镜
        let mut min_filtered_img: ImageBuffer<image::Luma<u8>, Vec<u8>> =
            ImageBuffer::new(width, height);
        let radius = (kernel_size / 2) as i32;

        for x in 0..width {
            for y in 0..height {
                let mut min_val = 255u8;

                // 遍历kernel范围内的像素
                for kx in -radius..=radius {
                    for ky in -radius..=radius {
                        let nx = (x as i32 + kx).max(0).min((width - 1) as i32) as u32;
                        let ny = (y as i32 + ky).max(0).min((height - 1) as i32) as u32;

                        let pix = inverted_gray_image.get_pixel(nx, ny);
                        min_val = min_val.min(pix.0[0]);
                    }
                }

                min_filtered_img.put_pixel(x, y, image::Luma([min_val]));
            }
        }
        // 颜色减淡
        let mut pencil_sketch: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::new(width, height);
        for x in 0..width {
            for y in 0..height {
                let image_pix = gray_image
                    .get_pixel_checked(x, y)
                    .ok_or("image out of bound")?;
                let mask_pix = min_filtered_img
                    .get_pixel_checked(x, y)
                    .ok_or("image out of bound")?;
                if mask_pix.0[0] == 255 {
                    // 防止除以0
                    pencil_sketch.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                } else {
                    let val = image_pix.0[0] as f32 / (255 - mask_pix.0[0]) as f32 * 256.0;
                    let u8_val = if val > 255.0 { 255 } else { val as u8 };
                    pencil_sketch.put_pixel(x, y, image::Rgba([u8_val, u8_val, u8_val, 255]));
                }
            }
        }
        // 对比度增强（针对黑底白线）
        if contrast != 1.0 {
            for x in 0..width {
                for y in 0..height {
                    let pix = pencil_sketch.get_pixel(x, y);
                    let val = pix.0[0] as f32 / 255.0; // 归一化到 [0, 1]

                    // 使用幂函数调整对比度
                    // contrast > 1: 压暗暗部，提亮亮部（增强对比）
                    // contrast < 1: 提亮暗部，压暗亮部（降低对比）
                    let new_val = val.powf(1.0 / contrast);
                    let clamped_val = (new_val * 255.0).clamp(0.0, 255.0) as u8;

                    pencil_sketch.put_pixel(
                        x,
                        y,
                        image::Rgba([clamped_val, clamped_val, clamped_val, 255]),
                    );
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&pencil_sketch)?;
        Ok(Some(rv_bin(ret)))
    });

    add_fun(vec!["图片蒙版", "图像蒙版"], |self_t, params| {
        let (_img_fmt, mut img_big) = get_param_image(self_t, params, 0)?;
        let (_img_fmt, img_sub) = get_param_image(self_t, params, 1)?;
        let text3 = self_t.get_param_text(params, 2)?;
        let text4 = self_t.get_param_text(params, 3)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        for i in 0..img_sub.width() {
            for j in 0..img_sub.height() {
                let ii = x + i as i64;
                let jj = y + j as i64;
                if ii >= 0
                    && (ii as u32) < img_big.width()
                    && jj >= 0
                    && (jj as u32) < img_big.height()
                {
                    let pix = img_big
                        .get_pixel_mut_checked(ii as u32, jj as u32)
                        .ok_or("image out of bound")?;
                    let pix_sub = img_sub
                        .get_pixel_checked(i, j)
                        .ok_or("image out of bound")?;
                    // 先将蒙版变成灰度图
                    let color = cala_gray_val(pix_sub.0[0], pix_sub.0[1], pix_sub.0[2]);
                    pix.0[3] = color;
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img_big)?;
        Ok(Some(rv_bin(ret)))
    });

    fn get_char_size(font: &rusttype::Font, scale: Scale, ch: char) -> (i32, i32) {
        let v_metrics = font.v_metrics(scale);
        let text = ch.to_string();
        let mut width = 0;
        let mut height = 0;
        for g in font.layout(&text, scale, rusttype::point(0.0, v_metrics.ascent)) {
            let h_metrics = font.glyph(g.id()).scaled(scale).h_metrics();
            width = (h_metrics.advance_width + 0.5) as i32;
            if let Some(bb) = g.pixel_bounding_box() {
                height = bb.max.y;
            }
        }
        (width, height)
    }
    add_fun(
        vec!["竖文字转图片", "竖文字转图像"],
        |self_t, params| {
            let image_height = self_t.get_param_text(params, 0)?.parse::<u32>()?;
            let text = self_t.get_param_text(params, 1)?;
            let text_size = self_t.get_param_text(params, 2)?.parse::<f32>()?;
            let text_color = get_param_text_array(self_t, params, 3)?;
            let mut color = Rgba::<u8>([0, 0, 0, 255]);
            color.0[0] = text_color
                .first()
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[1] = text_color
                .get(1)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[2] = text_color
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[3] = text_color
                .get(3)
                .map(|s| s.as_str())
                .unwrap_or("255")
                .parse::<u8>()?;
            let scale = Scale::uniform(text_size);

            let font_dat = self_t.get_param_bin(params, 4)?;
            let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
            let font_t = FontRef::try_from_slice(&font_dat)?;
            let font_sep_text = self_t.get_param_text(params, 5)?;
            let line_sep_text = self_t.get_param_text(params, 6)?;
            let font_sep = if !font_sep_text.is_empty() {
                font_sep_text.parse::<usize>()?
            } else {
                0usize
            };
            let line_sep = if !line_sep_text.is_empty() {
                line_sep_text.parse::<usize>()?
            } else {
                0usize
            };
            let image_width;
            {
                let mut max_x = 0;
                let text_chars = text.chars().collect::<Vec<char>>();
                let mut cur_x = 0;
                let mut cur_y = 0;
                for ch in text_chars {
                    let (width, height) = get_char_size(&font, scale, ch);
                    if width > max_x {
                        max_x = width;
                    }
                    if ch == ' ' {
                        if cur_y + ((text_size / 2. + 0.5) as i32) < image_height as i32 {
                            cur_y += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                        } else {
                            cur_x += max_x + line_sep as i32;
                            cur_y = 0;
                            cur_y += (text_size as i32) + font_sep as i32;
                        }
                    } else if ch == '\n' {
                        cur_y = 0;
                        cur_x += max_x + line_sep as i32;
                    } else {
                        if cur_y + height < image_height as i32 {
                            cur_y += height + font_sep as i32;
                        } else {
                            cur_x += max_x + line_sep as i32;
                            cur_y = 0;
                            cur_y += height + font_sep as i32;
                        }
                    }
                }
                cur_x += max_x + line_sep as i32;
                image_width = cur_x;
            }
            check_image_dims(image_width as usize, image_height as usize)?;
            let mut img = ImageBuffer::new(image_width as u32, image_height);
            {
                let mut max_x = 0;
                let text_chars = text.chars().collect::<Vec<char>>();
                let mut cur_x = image_width;
                let mut cur_y = 0;
                for ch in text_chars {
                    let (width, height) = get_char_size(&font, scale, ch);
                    if width > max_x {
                        max_x = width;
                    }
                    if ch == ' ' {
                        if cur_y + ((text_size / 2. + 0.5) as i32) < img.height() as i32 {
                            cur_y += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                        } else {
                            cur_x -= max_x + line_sep as i32;
                            cur_y = 0;
                            cur_y += (text_size as i32) + font_sep as i32;
                        }
                    } else if ch == '\n' {
                        cur_y = 0;
                        cur_x -= max_x + line_sep as i32;
                    } else {
                        if cur_y + height < img.height() as i32 {
                            imageproc::drawing::draw_text_mut(
                                &mut img,
                                color,
                                cur_x - width,
                                cur_y,
                                PxScale {
                                    x: scale.x,
                                    y: scale.y,
                                },
                                &font_t,
                                &ch.to_string(),
                            );
                            cur_y += height + font_sep as i32;
                        } else {
                            cur_x -= max_x + line_sep as i32;
                            cur_y = 0;
                            imageproc::drawing::draw_text_mut(
                                &mut img,
                                color,
                                cur_x - width,
                                cur_y,
                                PxScale {
                                    x: scale.x,
                                    y: scale.y,
                                },
                                &font_t,
                                &ch.to_string(),
                            );
                            cur_y += height + font_sep as i32;
                        }
                    }
                }
            }
            let ret = self_t.build_bin_raw_from_img(&img)?;
            Ok(Some(rv_bin(ret)))
        },
    );
    add_fun(
        vec!["文字转图片", "文字转图像"],
        |self_t, params| {
            let image_width = self_t.get_param_text(params, 0)?.parse::<u32>()?;
            let text = self_t.get_param_text(params, 1)?;
            let text_size = self_t.get_param_text(params, 2)?.parse::<f32>()?;            let text_color = get_param_text_array(self_t, params, 3)?;
            let mut color = Rgba::<u8>([0, 0, 0, 255]);
            color.0[0] = text_color
                .first()
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[1] = text_color
                .get(1)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[2] = text_color
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("0")
                .parse::<u8>()?;
            color.0[3] = text_color
                .get(3)
                .map(|s| s.as_str())
                .unwrap_or("255")
                .parse::<u8>()?;
            let scale = Scale::uniform(text_size);

            let font_dat = self_t.get_param_bin(params, 4)?;
            let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
            let font_t = FontRef::try_from_slice(&font_dat)?;
            let font_sep_text = self_t.get_param_text(params, 5)?;
            let line_sep_text = self_t.get_param_text(params, 6)?;
            let font_sep = if !font_sep_text.is_empty() {
                font_sep_text.parse::<usize>()?
            } else {
                0usize
            };
            let line_sep = if !line_sep_text.is_empty() {
                line_sep_text.parse::<usize>()?
            } else {
                0usize
            };
            let image_height;
            {
                let mut max_y = 0;
                let text_chars = text.chars().collect::<Vec<char>>();
                let mut cur_x = 0;
                let mut cur_y = 0;
                for ch in text_chars {
                    let (width, height) = get_char_size(&font, scale, ch);
                    if height > max_y {
                        max_y = height;
                    }
                    if ch == ' ' {
                        if cur_x + ((text_size / 2. + 0.5) as i32) < image_width as i32 {
                            cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                        } else {
                            cur_y += max_y + line_sep as i32;
                            cur_x = 0;
                            cur_x += (text_size as i32) + font_sep as i32;
                        }
                    } else if ch == '\n' {
                        cur_x = 0;
                        cur_y += max_y + line_sep as i32;
                    } else {
                        if cur_x + width < image_width as i32 {
                            cur_x += width + font_sep as i32;
                        } else {
                            cur_y += max_y + line_sep as i32;
                            cur_x = 0;
                            cur_x += width + font_sep as i32;
                        }
                    }
                }
                cur_y += max_y + line_sep as i32;
                image_height = cur_y;
            }
            check_image_dims(image_width as usize, image_height.max(0) as usize)?;
            let mut img = ImageBuffer::new(image_width, image_height as u32);
            {
                let mut max_y = 0;
                let text_chars = text.chars().collect::<Vec<char>>();
                let mut cur_x = 0;
                let mut cur_y = 0;
                for ch in text_chars {
                    let (width, height) = get_char_size(&font, scale, ch);
                    if height > max_y {
                        max_y = height;
                    }
                    if ch == ' ' {
                        if cur_x + ((text_size / 2. + 0.5) as i32) < img.width() as i32 {
                            cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                        } else {
                            cur_y += max_y + line_sep as i32;
                            cur_x = 0;
                            cur_x += (text_size as i32) + font_sep as i32;
                        }
                    } else if ch == '\n' {
                        cur_x = 0;
                        cur_y += max_y + line_sep as i32;
                    } else {
                        if cur_x + width < img.width() as i32 {
                            imageproc::drawing::draw_text_mut(
                                &mut img,
                                color,
                                cur_x,
                                cur_y,
                                PxScale {
                                    x: scale.x,
                                    y: scale.y,
                                },
                                &font_t,
                                &ch.to_string(),
                            );
                            cur_x += width + font_sep as i32;
                        } else {
                            cur_y += max_y + line_sep as i32;
                            cur_x = 0;
                            imageproc::drawing::draw_text_mut(
                                &mut img,
                                color,
                                cur_x,
                                cur_y,
                                PxScale {
                                    x: scale.x,
                                    y: scale.y,
                                },
                                &font_t,
                                &ch.to_string(),
                            );
                            cur_x += width + font_sep as i32;
                        }
                    }
                }
            }
            let ret = self_t.build_bin_raw_from_img(&img)?;
            Ok(Some(rv_bin(ret)))
        },
    );
    add_fun(vec!["图片嵌字", "图像嵌字"], |self_t, params| {
        let (_, mut img) = get_param_image(self_t, params, 0)?;
        let text = self_t.get_param_text(params, 1)?;
        let text_x = self_t.get_param_text(params, 2)?.parse::<i32>()?;
        let text_y = self_t.get_param_text(params, 3)?.parse::<i32>()?;
        let text_size = self_t.get_param_text(params, 4)?.parse::<f32>()?;
        let text_color = get_param_text_array(self_t, params, 5)?;
        let mut color = Rgba::<u8>([0, 0, 0, 255]);
        color.0[0] = text_color
            .first()
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[1] = text_color
            .get(1)
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[2] = text_color
            .get(2)
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[3] = text_color
            .get(3)
            .map(|s| s.as_str())
            .unwrap_or("255")
            .parse::<u8>()?;
        let scale = Scale {
            x: text_size,
            y: text_size,
        };
        let font_dat = self_t.get_param_bin(params, 6)?;
        let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
        let font_t = FontRef::try_from_slice(&font_dat)?;
        let font_sep_text = self_t.get_param_text(params, 7)?;
        let line_sep_text = self_t.get_param_text(params, 8)?;
        let font_sep = if !font_sep_text.is_empty() {
            font_sep_text.parse::<usize>()?
        } else {
            0usize
        };
        let line_sep = if !line_sep_text.is_empty() {
            line_sep_text.parse::<usize>()?
        } else {
            0usize
        };
        {
            let mut max_y = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = 0;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width, height) = get_char_size(&font, scale, ch);
                if height > max_y {
                    max_y = height;
                }
                if ch == ' ' {
                    if text_x + cur_x + ((text_size / 2. + 0.5) as i32) < img.width() as i32 {
                        cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        cur_x += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_x = 0;
                    cur_y += max_y + line_sep as i32;
                } else {
                    if text_x + cur_x + width < img.width() as i32 {
                        imageproc::drawing::draw_text_mut(
                            &mut img,
                            color,
                            text_x + cur_x,
                            text_y + cur_y,
                            PxScale {
                                x: scale.x,
                                y: scale.y,
                            },
                            &font_t,
                            &ch.to_string(),
                        );
                        cur_x += width + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        imageproc::drawing::draw_text_mut(
                            &mut img,
                            color,
                            text_x + cur_x,
                            text_y + cur_y,
                            PxScale {
                                x: scale.x,
                                y: scale.y,
                            },
                            &font_t,
                            &ch.to_string(),
                        );
                        cur_x += width + font_sep as i32;
                    }
                }
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(vec!["创建图片", "创建图像"], |self_t, params| {
        let image_width = self_t.get_param_text(params, 0)?.parse::<u32>()?;
        let image_height = self_t.get_param_text(params, 1)?.parse::<u32>()?;
        check_image_dims(image_width as usize, image_height as usize)?;
        let text_color = get_param_text_array(self_t, params, 2)?;
        let mut color = Rgba::<u8>([0, 0, 0, 0]);
        color.0[0] = text_color
            .first()
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[1] = text_color
            .get(1)
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[2] = text_color
            .get(2)
            .map(|s| s.as_str())
            .unwrap_or("0")
            .parse::<u8>()?;
        color.0[3] = text_color
            .get(3)
            .map(|s| s.as_str())
            .unwrap_or("255")
            .parse::<u8>()?;
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(image_width, image_height);
        for x in 0..image_width {
            for y in 0..image_height {
                let pix = img
                    .get_pixel_mut_checked(x, y)
                    .ok_or("image out of bound")?;
                pix.0 = color.0;
            }
        }
        let ret = self_t.build_bin_raw_from_img(&img)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(vec!["水平翻转"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let img_out = image::imageops::flip_horizontal(&img);
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(vec!["垂直翻转"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let img_out = image::imageops::flip_vertical(&img);
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(vec!["图像旋转", "图片旋转"], |self_t, params| {
        let (_, img) = get_param_image(self_t, params, 0)?;
        let text2 = self_t.get_param_text(params, 1)?;
        let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
        let img_out = rotate_about_center(&img, theta, Interpolation::Bilinear, Rgba([0, 0, 0, 0]));
        let ret = self_t.build_bin_raw_from_img(&img_out)?;
        Ok(Some(rv_bin(ret)))
    });
    add_fun(
        vec!["完整图像旋转", "完整图片旋转"],
        |self_t, params| {
            let (_, img_sub) = get_param_image(self_t, params, 0)?;
            let text2 = self_t.get_param_text(params, 1)?;
            let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
            let img_width = img_sub.width();
            let img_height = img_sub.height();
            let (sin_val, cos_val) = theta.sin_cos();
            let vec1 = (img_width as f32 / 2.0, img_height as f32 / 2.0);
            let vec2 = (img_width as f32 / 2.0, -(img_height as f32 / 2.0));
            let vec1_t = (
                vec1.0 * cos_val - vec1.1 * sin_val,
                vec1.0 * sin_val + vec1.1 * cos_val,
            );
            let vec2_t = (
                vec2.0 * cos_val - vec2.1 * sin_val,
                vec2.0 * sin_val + vec2.1 * cos_val,
            );
            let max_width = std::cmp::max(vec1_t.0.abs() as i64, vec2_t.0.abs() as i64) as u32;
            let max_height = std::cmp::max(vec1_t.1.abs() as i64, vec2_t.1.abs() as i64) as u32;
            let max_wh = ((img_width * img_width + img_height * img_height) as f64).sqrt() as u32;
            let mut img_big: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(max_wh, max_wh);
            image::imageops::overlay(
                &mut img_big,
                &img_sub,
                ((max_wh - img_width) / 2).into(),
                ((max_wh - img_height) / 2).into(),
            );
            let mut img_out_t =
                rotate_about_center(&img_big, theta, Interpolation::Bilinear, Rgba([0, 0, 0, 0]));
            let img_out = image::imageops::crop(
                &mut img_out_t,
                (max_wh - max_width * 2) / 2,
                (max_wh - max_height * 2) / 2,
                max_width * 2,
                max_height * 2,
            )
            .to_image();
            let ret = self_t.build_bin_raw_from_img(&img_out)?;
            Ok(Some(rv_bin(ret)))
        },
    );
    add_fun(
        vec!["图像大小调整", "图片大小调整"],
        |self_t, params| {
            let (_, img) = get_param_image(self_t, params, 0)?;
            let mut width = self_t.get_param_text(params, 1)?.parse::<u32>()?;
            let mut height = self_t.get_param_text(params, 2)?.parse::<u32>()?;
            if width == 0 && height != 0 {
                let k = (height as f64) / (img.height() as f64);
                width = ((img.width() as f64) * k).round() as u32;
            } else if width != 0 && height == 0 {
                let k = (width as f64) / (img.width() as f64);
                height = ((img.height() as f64) * k).round() as u32;
            } else if width == 0 && height == 0 {
                return Err(RedLang::make_err("目标图片高和宽均为0，无法调整大小"));
            }
            check_image_dims(width as usize, height as usize)?;
            let img_out =
                image::imageops::resize(&img, width, height, image::imageops::FilterType::Nearest);
            let ret = self_t.build_bin_raw_from_img(&img_out)?;
            Ok(Some(rv_bin(ret)))
        },
    );
}
