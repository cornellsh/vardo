// Vite serves images as URL strings; TypeScript needs telling.
declare module "*.png" {
	const src: string;
	export default src;
}
