const hostname = window.location.hostname.replace(/^www\./, '')
const localHost = hostname === 'localhost' || /^[\d.]+$/.test(hostname) || hostname.includes(':')
const configuredDemoUrl = document.querySelector('meta[name="bokhylle-demo-url"]')?.content.trim()
const demoUrl = configuredDemoUrl || (localHost
  ? `http://${window.location.hostname}:8081`
  : `https://demo.${hostname}`)

for (const link of document.querySelectorAll('.demo-link')) {
  link.href = demoUrl
}
