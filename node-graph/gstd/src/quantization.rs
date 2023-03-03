use dyn_any::{DynAny, StaticType};
use graphene_core::raster::{Color, ImageFrame};
use graphene_core::Node;

/// The `GenerateQuantizationNode` encodes the brightness of each channel of the image as an integer number
/// sepified by the samples parameter. This node is used to asses the loss of visual information when
/// quantizing the image using different fit functions.
pub struct GenerateQuantizationNode<N, M> {
	samples: N,
	function: M,
}

#[node_macro::node_fn(GenerateQuantizationNode)]
fn generate_quantization_fn(image_frame: ImageFrame, samples: u32, function: u32) -> Quantization {
	let image = image_frame.image;
	// Scale the input image, this can be removed by adding an extra parameter to the fit function.
	let max_energy = 16380.;
	let data: Vec<f64> = image.data.iter().flat_map(|x| vec![x.r() as f64, x.g() as f64, x.b() as f64]).collect();
	let data: Vec<f64> = data.iter().map(|x| x * max_energy).collect();
	let mut dist = autoquant::integrate_distribution(data);
	autoquant::drop_duplicates(&mut dist);
	let dist = autoquant::normalize_distribution(dist.as_slice());
	let max = dist.last().unwrap().0;
	/*let linear = Box::new(autoquant::SimpleFitFn {
		function: move |x| x / max,
		inverse: move |x| x * max,
		name: "identity",
	});*/

	let linear = Quantization {
		fn_index: 0,
		a: max as f32,
		b: 0.,
		c: 0.,
		d: 0.,
	};
	let log_fit = autoquant::models::OptimizedLog::new(dist, samples as u64);
	let parameters = log_fit.parameters();
	let log_fit = Quantization {
		fn_index: 1,
		a: parameters[0] as f32,
		b: parameters[1] as f32,
		c: parameters[2] as f32,
		d: parameters[3] as f32,
	};
	log_fit
}

#[derive(Clone, Debug, DynAny)]
pub struct Quantization {
	fn_index: usize,
	a: f32,
	b: f32,
	c: f32,
	d: f32,
}

fn quantize(value: f32, quantization: &Quantization) -> f32 {
	let Quantization { fn_index, a, b, c, d } = quantization;
	match fn_index {
		1 => ((value + a) * d).abs().ln() * b + c,
		_ => a * value + b,
	}
}

fn decode(value: f32, quantization: &Quantization) -> f32 {
	let Quantization { fn_index, a, b, c, d } = quantization;
	match fn_index {
		1 => -(-c / b).exp() * (a * d * (c / b).exp() - (value / b).exp()) / d,
		_ => (value - b) / a,
	}
}

pub struct QuantizeNode<Quantization> {
	quantization: Quantization,
}

#[node_macro::node_fn(QuantizeNode)]
fn quantize_fn<'a>(color: Color, quantization: &'a [Quantization; 4]) -> Color {
	let quant = quantization.as_slice();
	let r = quantize(color.r(), &quant[0]);
	let g = quantize(color.g(), &quant[1]);
	let b = quantize(color.b(), &quant[2]);
	let a = quantize(color.a(), &quant[3]);

	Color::from_rgbaf32_unchecked(r, g, b, a)
}

pub struct DeQuantizeNode<Quantization> {
	quantization: Quantization,
}

#[node_macro::node_fn(DeQuantizeNode)]
fn dequantize_fn<'a>(color: Color, quantization: &'a [Quantization; 4]) -> Color {
	let quant = quantization.as_slice();
	let r = decode(color.r(), &quant[0]);
	let g = decode(color.g(), &quant[1]);
	let b = decode(color.b(), &quant[2]);
	let a = decode(color.a(), &quant[3]);

	Color::from_rgbaf32_unchecked(r, g, b, a)
}
