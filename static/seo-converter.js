(() => {
  const language = document.documentElement.lang.toLowerCase().split("-")[0];
  const messages = {
    fr: { locale: "fr-FR", count: "caractères", rendering: "Génération du PDF…", ready: "Votre PDF est prêt.", download: "Télécharger le PDF", bad: "La réponse reçue n’est pas un PDF.", failed: "La conversion a échoué : " },
    en: { locale: "en-US", count: "characters", rendering: "Generating your PDF…", ready: "Your PDF is ready.", download: "Download the PDF", bad: "The response is not a PDF.", failed: "Conversion failed: " },
    es: { locale: "es-ES", count: "caracteres", rendering: "Generando el PDF…", ready: "Tu PDF está listo.", download: "Descargar el PDF", bad: "La respuesta recibida no es un PDF.", failed: "La conversión ha fallado: " },
    de: { locale: "de-DE", count: "Zeichen", rendering: "PDF wird erstellt…", ready: "Dein PDF ist fertig.", download: "PDF herunterladen", bad: "Die Antwort ist keine PDF-Datei.", failed: "Umwandlung fehlgeschlagen: " },
    it: { locale: "it-IT", count: "caratteri", rendering: "Creazione del PDF…", ready: "Il PDF è pronto.", download: "Scarica il PDF", bad: "La risposta ricevuta non è un PDF.", failed: "Conversione non riuscita: " },
    pt: { locale: "pt-PT", count: "caracteres", rendering: "A gerar o PDF…", ready: "O seu PDF está pronto.", download: "Descarregar o PDF", bad: "A resposta recebida não é um PDF.", failed: "A conversão falhou: " },
  };
  const words = messages[language] || messages.en;
  const form = document.querySelector("#converter");
  if (!form) return;
  const input = document.querySelector("#markdown");
  const button = document.querySelector("#convert");
  const status = document.querySelector("#status");
  const download = document.querySelector("#download");
  const counter = document.querySelector("#counter");
  let previousUrl = null;

  input.addEventListener("input", () => {
    const count = input.value.length.toLocaleString(words.locale);
    const limit = (100000).toLocaleString(words.locale);
    counter.textContent = `${count} / ${limit} ${words.count}`;
  });
  input.dispatchEvent(new Event("input"));

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    button.disabled = true;
    download.hidden = true;
    status.textContent = words.rendering;
    status.className = "status";
    if (previousUrl) URL.revokeObjectURL(previousUrl);

    try {
      const body = new FormData();
      body.set("markdown", input.value);
      const response = await fetch("/", { method: "POST", body });
      if (!response.ok) {
        const detail = (await response.text()).slice(0, 240);
        throw new Error(detail || `HTTP ${response.status}`);
      }
      const blob = await response.blob();
      if (blob.type !== "application/pdf") throw new Error(words.bad);
      previousUrl = URL.createObjectURL(blob);
      download.href = previousUrl;
      download.download = "document.pdf";
      download.textContent = words.download;
      download.hidden = false;
      status.textContent = words.ready;
      status.classList.add("success");
    } catch (error) {
      status.textContent = words.failed + error.message;
      status.classList.add("error");
    } finally {
      button.disabled = false;
    }
  });
})();
