#!/usr/bin/env python3
"""Build localized public product pages from the reviewed English HTML templates."""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
LOCALES_FILE = ROOT / "content" / "blog" / "locales.json"
SITE_URL = "https://pdf.aismarttalk.tech"
LOCALES = json.loads(LOCALES_FILE.read_text(encoding="utf-8"))
BY_CODE = {item["code"]: item for item in LOCALES["languages"]}
DEFAULT_LANGUAGE = LOCALES["default_language"]


UI = {
    "es": {
        "main": "Navegación principal", "languages": "Idiomas", "convert": "Convertir",
        "convert_online": "Convertir en línea", "guides": "Guías", "api": "API",
        "api_reference": "Referencia de API", "developer_access": "Acceso para desarrolladores",
        "request_access": "Solicitar acceso", "footer_converter": "Convertidor",
    },
    "de": {
        "main": "Hauptnavigation", "languages": "Sprachen", "convert": "Konvertieren",
        "convert_online": "Online konvertieren", "guides": "Leitfäden", "api": "API",
        "api_reference": "API-Referenz", "developer_access": "Entwicklerzugang",
        "request_access": "Zugang anfragen", "footer_converter": "Konverter",
    },
    "it": {
        "main": "Navigazione principale", "languages": "Lingue", "convert": "Converti",
        "convert_online": "Converti online", "guides": "Guide", "api": "API",
        "api_reference": "Documentazione API", "developer_access": "Accesso sviluppatori",
        "request_access": "Richiedi l’accesso", "footer_converter": "Convertitore",
    },
    "pt": {
        "main": "Navegação principal", "languages": "Idiomas", "convert": "Converter",
        "convert_online": "Converter online", "guides": "Guias", "api": "API",
        "api_reference": "Referência da API", "developer_access": "Acesso para desenvolvedores",
        "request_access": "Solicitar acesso", "footer_converter": "Conversor",
    },
}


COPY = {
    "es": {
        "converter": {
            "Convert Markdown to PDF Online": "Convertir Markdown a PDF en línea",
            "A simple Markdown to PDF converter with a direct download.": "Un convertidor sencillo de Markdown a PDF con descarga directa.",
            "Turn Markdown into a downloadable PDF in your browser. Keep headings, tables, links and code readable with md-to-pdf by AI SmartTalk.": "Convierte Markdown en un PDF descargable desde el navegador. Conserva la claridad de los títulos, las tablas, los enlaces y el código con md-to-pdf de AI SmartTalk.",
            "Convert Markdown to PDF in a few clicks": "Convierte Markdown a PDF en pocos clics",
            "Paste your Markdown, create a PDF and download it. Headings, tables, links and code blocks are rendered into a document you can share.": "Pega tu Markdown, crea un PDF y descárgalo. Los títulos, las tablas, los enlaces y los bloques de código se convierten en un documento listo para compartir.",
            "ONLINE TOOL": "HERRAMIENTA EN LÍNEA",
            "PDF CONVERSION · AI SMARTTALK": "CONVERSIÓN A PDF · AI SMARTTALK",
            "Your Markdown document": "Tu documento Markdown",
            "No account": "Sin cuenta",
            "Markdown content": "Contenido Markdown",
            "0 / 100,000 characters": "0 / 100.000 caracteres",
            "Create my PDF": "Crear mi PDF",
            "Download the PDF": "Descargar el PDF",
            "Your document is processed for conversion. This tool returns the PDF directly, disables the shared cache and removes temporary files after the response.": "Tu documento se procesa para convertirlo. La herramienta devuelve el PDF directamente, desactiva la caché compartida y elimina los archivos temporales al terminar la respuesta.",
            "How to convert Markdown to PDF": "Cómo convertir Markdown a PDF",
            "Paste or write your document in the editor.": "Pega o escribe el documento en el editor.",
            "Select": "Pulsa",
            "and wait for rendering.": "y espera a que se genere el PDF.",
            "Download the generated PDF.": "Descarga el PDF generado.",
            "The converter supports common Markdown headings, lists, links, tables, quotes and code blocks. For branded layouts, JSON data and automated workflows, see the ": "El convertidor admite títulos, listas, enlaces, tablas, citas y bloques de código habituales en Markdown. Para usar diseños de marca, datos JSON y flujos automatizados, consulta la ",
            "PDF API for developers": "API de PDF para desarrolladores",
            "Need to automate?": "¿Necesitas automatizarlo?",
            "Use the md-to-pdf API to create branded documents from your applications and workflows.": "Usa la API de md-to-pdf para crear documentos con tu marca desde tus aplicaciones y flujos de trabajo.",
            "Explore the API and examples →": "Descubre la API y sus ejemplos →",
            "Frequently asked questions": "Preguntas frecuentes",
            "Do I need an account?": "¿Necesito una cuenta?",
            "No account is needed for this quick conversion. API access requires a key so integrations can be protected and usage attributed.": "No necesitas una cuenta para esta conversión rápida. El acceso a la API requiere una clave para proteger las integraciones y atribuir su uso.",
            "Which Markdown elements are supported?": "¿Qué elementos de Markdown se admiten?",
            "Common headings, paragraphs, lists, links, quotes, tables and code blocks are converted to PDF.": "Los títulos, párrafos, listas, enlaces, citas, tablas y bloques de código habituales se convierten en PDF.",
            "Is the PDF stored after conversion?": "¿Se guarda el PDF después de convertirlo?",
            "No. This page returns the PDF directly and skips shared disk storage. External resources referenced by your document may be fetched by the renderer.": "No. Esta página devuelve el PDF directamente y evita guardarlo en el almacenamiento compartido. El motor de conversión podría acceder a recursos externos enlazados en el documento.",
            "More from md-to-pdf": "Más recursos de md-to-pdf",
            "Read PDF generation guides": "Lee las guías de generación de PDF",
            "Integrate the PDF API": "Integra la API de PDF",
            "Open the application": "Abrir la aplicación",
            "Online Markdown to PDF converter with direct download.": "Convertidor en línea de Markdown a PDF con descarga directa.",
        },
        "api": {
            "PDF Generation API for Developers": "API de generación de PDF para desarrolladores",
            "Create PDFs from Markdown, HTML and JSON templates with the md-to-pdf API by AI SmartTalk. Explore curl examples, brand themes and secure API key access.": "Crea PDF a partir de Markdown, HTML y plantillas JSON con la API md-to-pdf de AI SmartTalk. Consulta ejemplos con curl, diseños de marca y acceso seguro mediante claves de API.",
            "Turn Markdown, HTML and structured data into production PDFs from your app.": "Convierte Markdown, HTML y datos estructurados en PDF listos para producción desde tu aplicación.",
            "PDF generation API for Markdown, HTML and JSON data.": "API de generación de PDF a partir de Markdown, HTML y datos JSON.",
            "FOR PRODUCT AND ENGINEERING TEAMS": "PARA EQUIPOS DE PRODUCTO E INGENIERÍA",
            "Generate PDFs from your app with an API": "Genera PDF desde tu aplicación con una API",
            "Send Markdown, HTML or structured data. Get back a PDF ready to deliver, with brand themes, previews and document quality tools.": "Envía Markdown, HTML o datos estructurados y recibe un PDF listo para entregar, con diseños de marca, vistas previas y herramientas para revisar la calidad del documento.",
            "Request an API key →": "Solicitar una clave de API →",
            "Browse the reference": "Consultar la referencia",
            "Markdown to PDF": "Markdown a PDF",
            "Send a report, proposal or meeting notes in Markdown, with CSS and page options.": "Envía informes, propuestas o actas en Markdown y configura CSS y opciones de página.",
            "Data to PDF": "Datos a PDF",
            "Fill a Tera template with JSON data to create personalized documents repeatedly.": "Rellena una plantilla Tera con datos JSON para generar documentos personalizados de forma recurrente.",
            "Inspect the output": "Revisa el resultado",
            "Preview pages, compare versions and audit layouts before delivery.": "Previsualiza páginas, compara versiones y revisa el diseño antes de entregar el documento.",
            "API capabilities": "Funciones de la API",
            "Example: convert a Markdown document": "Ejemplo: convertir un documento Markdown",
            "Every API request uses a customer-specific key in the ": "Cada solicitud a la API utiliza una clave específica del cliente en la cabecera ",
            "header. Administrators can create and revoke each key independently.": ". Los administradores pueden crear y revocar cada clave por separado.",
            "Keep the key on your server or in your secrets manager. Never ship it in browser JavaScript.": "Guarda la clave en tu servidor o en un gestor de secretos. Nunca la incluyas en el JavaScript del navegador.",
            "Try the public converter": "Prueba el convertidor público",
            "Preview how a Markdown document renders before connecting an integration.": "Comprueba cómo se convierte un documento Markdown antes de conectar una integración.",
            "Open the Markdown to PDF tool →": "Abrir la herramienta de Markdown a PDF →",
            "One engine, multiple workflows": "Un motor para varios flujos de trabajo",
            "The service brings together conversion, template rendering, preview, merge, watermark, password protection, redaction and layout analysis. API operations require a key so access stays protected and usage can be attributed to each integration.": "El servicio reúne conversión, generación desde plantillas, vista previa, combinación, marcas de agua, protección con contraseña, redacción de datos y análisis de diseño. Las operaciones de la API requieren una clave para proteger el acceso y atribuir el uso a cada integración.",
            "Browse every endpoint in the interactive reference": "Consulta todos los endpoints en la referencia interactiva",
            " or ": " o ",
            "request an API key": "solicitar una clave de API",
            "Guides and examples": "Guías y ejemplos",
            "Read integration guides": "Lee las guías de integración",
            "Convert an example online": "Convierte un ejemplo en línea",
        },
        "sample": "# Mi documento\n\nEscribe en **Markdown**, añade una lista o una tabla y crea tu PDF.\n\n| Elemento | Ejemplo |\n| --- | --- |\n| Título | `# Informe` |\n| Lista | `- Primer paso` |\n| Enlace | `[AI SmartTalk](https://aismarttalk.tech)` |\n\n```text\nLos bloques de código se leen con claridad en el PDF.\n```",
    },
    "de": {
        "converter": {
            "Convert Markdown to PDF Online": "Markdown online in PDF umwandeln",
            "A simple Markdown to PDF converter with a direct download.": "Ein einfacher Markdown-zu-PDF-Konverter mit direktem Download.",
            "Turn Markdown into a downloadable PDF in your browser. Keep headings, tables, links and code readable with md-to-pdf by AI SmartTalk.": "Wandle Markdown direkt im Browser in ein herunterladbares PDF um. Überschriften, Tabellen, Links und Code bleiben mit md-to-pdf von AI SmartTalk gut lesbar.",
            "Convert Markdown to PDF in a few clicks": "Markdown mit wenigen Klicks in PDF umwandeln",
            "Paste your Markdown, create a PDF and download it. Headings, tables, links and code blocks are rendered into a document you can share.": "Füge dein Markdown ein, erstelle ein PDF und lade es herunter. Überschriften, Tabellen, Links und Codeblöcke werden in ein Dokument zum Teilen umgewandelt.",
            "ONLINE TOOL": "ONLINE-TOOL",
            "PDF CONVERSION · AI SMARTTALK": "PDF-UMWANDLUNG · AI SMARTTALK",
            "Your Markdown document": "Dein Markdown-Dokument",
            "No account": "Kein Konto nötig",
            "Markdown content": "Markdown-Inhalt",
            "0 / 100,000 characters": "0 / 100.000 Zeichen",
            "Create my PDF": "PDF erstellen",
            "Download the PDF": "PDF herunterladen",
            "Your document is processed for conversion. This tool returns the PDF directly, disables the shared cache and removes temporary files after the response.": "Dein Dokument wird für die Umwandlung verarbeitet. Das Tool liefert das PDF direkt zurück, deaktiviert den gemeinsamen Cache und entfernt temporäre Dateien nach der Antwort.",
            "How to convert Markdown to PDF": "Markdown in PDF umwandeln",
            "Paste or write your document in the editor.": "Füge dein Dokument in den Editor ein oder schreibe es dort.",
            "Select": "Klicke auf",
            "and wait for rendering.": "und warte, bis das PDF erstellt ist.",
            "Download the generated PDF.": "Lade das erstellte PDF herunter.",
            "The converter supports common Markdown headings, lists, links, tables, quotes and code blocks. For branded layouts, JSON data and automated workflows, see the ": "Der Konverter unterstützt gängige Markdown-Überschriften, Listen, Links, Tabellen, Zitate und Codeblöcke. Für Markenvorlagen, JSON-Daten und automatisierte Abläufe findest du weitere Informationen in der ",
            "PDF API for developers": "PDF-API für Entwickler",
            "Need to automate?": "Möchtest du Abläufe automatisieren?",
            "Use the md-to-pdf API to create branded documents from your applications and workflows.": "Mit der md-to-pdf-API erstellst du markengerechte Dokumente direkt aus deinen Anwendungen und Abläufen.",
            "Explore the API and examples →": "API und Beispiele ansehen →",
            "Frequently asked questions": "Häufige Fragen",
            "Do I need an account?": "Benötige ich ein Konto?",
            "No account is needed for this quick conversion. API access requires a key so integrations can be protected and usage attributed.": "Für diese schnelle Umwandlung brauchst du kein Konto. Der API-Zugang erfordert einen Schlüssel, damit Integrationen geschützt und Nutzungen zugeordnet werden können.",
            "Which Markdown elements are supported?": "Welche Markdown-Elemente werden unterstützt?",
            "Common headings, paragraphs, lists, links, quotes, tables and code blocks are converted to PDF.": "Gängige Überschriften, Absätze, Listen, Links, Zitate, Tabellen und Codeblöcke werden in PDF umgewandelt.",
            "Is the PDF stored after conversion?": "Wird das PDF nach der Umwandlung gespeichert?",
            "No. This page returns the PDF directly and skips shared disk storage. External resources referenced by your document may be fetched by the renderer.": "Nein. Diese Seite liefert das PDF direkt zurück und speichert es nicht auf dem gemeinsamen Datenträger. Externe Ressourcen, auf die dein Dokument verweist, können vom Renderer abgerufen werden.",
            "More from md-to-pdf": "Mehr von md-to-pdf",
            "Read PDF generation guides": "Leitfäden zur PDF-Erstellung lesen",
            "Integrate the PDF API": "PDF-API integrieren",
            "Open the application": "Anwendung öffnen",
            "Online Markdown to PDF converter with direct download.": "Online-Konverter für Markdown zu PDF mit direktem Download.",
        },
        "api": {
            "PDF Generation API for Developers": "PDF-Generierungs-API für Entwickler",
            "Create PDFs from Markdown, HTML and JSON templates with the md-to-pdf API by AI SmartTalk. Explore curl examples, brand themes and secure API key access.": "Erstelle mit der md-to-pdf-API von AI SmartTalk PDFs aus Markdown, HTML und JSON-Vorlagen. Entdecke curl-Beispiele, Markenvorlagen und den sicheren API-Schlüsselzugang.",
            "Turn Markdown, HTML and structured data into production PDFs from your app.": "Erstelle in deiner Anwendung produktionsfertige PDFs aus Markdown, HTML und strukturierten Daten.",
            "PDF generation API for Markdown, HTML and JSON data.": "PDF-Generierungs-API für Markdown, HTML und JSON-Daten.",
            "FOR PRODUCT AND ENGINEERING TEAMS": "FÜR PRODUKT- UND ENTWICKLUNGSTEAMS",
            "Generate PDFs from your app with an API": "PDFs per API direkt in deiner Anwendung erstellen",
            "Send Markdown, HTML or structured data. Get back a PDF ready to deliver, with brand themes, previews and document quality tools.": "Sende Markdown, HTML oder strukturierte Daten und erhalte ein versandfertiges PDF mit Markenvorlagen, Vorschauen und Werkzeugen zur Qualitätsprüfung.",
            "Request an API key →": "API-Schlüssel anfragen →",
            "Browse the reference": "Referenz ansehen",
            "Markdown to PDF": "Markdown zu PDF",
            "Send a report, proposal or meeting notes in Markdown, with CSS and page options.": "Sende Berichte, Angebote oder Besprechungsnotizen in Markdown und nutze CSS- und Seitenoptionen.",
            "Data to PDF": "Daten zu PDF",
            "Fill a Tera template with JSON data to create personalized documents repeatedly.": "Fülle eine Tera-Vorlage mit JSON-Daten aus, um wiederholt personalisierte Dokumente zu erstellen.",
            "Inspect the output": "Ergebnisse prüfen",
            "Preview pages, compare versions and audit layouts before delivery.": "Sieh dir Seiten vorab an, vergleiche Versionen und prüfe Layouts vor dem Versand.",
            "API capabilities": "API-Funktionen",
            "Example: convert a Markdown document": "Beispiel: ein Markdown-Dokument umwandeln",
            "Every API request uses a customer-specific key in the ": "Jede API-Anfrage verwendet einen kundenspezifischen Schlüssel im Header ",
            "header. Administrators can create and revoke each key independently.": ". Administratoren können jeden Schlüssel einzeln erstellen und widerrufen.",
            "Keep the key on your server or in your secrets manager. Never ship it in browser JavaScript.": "Speichere den Schlüssel auf deinem Server oder in einem Secret-Manager. Binde ihn niemals in JavaScript im Browser ein.",
            "Try the public converter": "Öffentlichen Konverter ausprobieren",
            "Preview how a Markdown document renders before connecting an integration.": "Sieh dir die Umwandlung eines Markdown-Dokuments an, bevor du eine Integration einrichtest.",
            "Open the Markdown to PDF tool →": "Markdown-zu-PDF-Tool öffnen →",
            "One engine, multiple workflows": "Eine Engine für viele Arbeitsabläufe",
            "The service brings together conversion, template rendering, preview, merge, watermark, password protection, redaction and layout analysis. API operations require a key so access stays protected and usage can be attributed to each integration.": "Der Dienst vereint Umwandlung, Vorlagen, Vorschau, Zusammenführen, Wasserzeichen, Passwortschutz, Schwärzen und Layoutanalyse. API-Aufrufe erfordern einen Schlüssel, damit der Zugang geschützt bleibt und die Nutzung einer Integration zugeordnet werden kann.",
            "Browse every endpoint in the interactive reference": "Alle Endpunkte in der interaktiven Referenz ansehen",
            " or ": " oder ",
            "request an API key": "einen API-Schlüssel anfragen",
            "Guides and examples": "Leitfäden und Beispiele",
            "Read integration guides": "Integrationsleitfäden lesen",
            "Convert an example online": "Ein Beispiel online umwandeln",
        },
        "sample": "# Mein Dokument\n\nSchreibe in **Markdown**, füge eine Liste oder Tabelle hinzu und erstelle dein PDF.\n\n| Element | Beispiel |\n| --- | --- |\n| Überschrift | `# Bericht` |\n| Liste | `- Erster Schritt` |\n| Link | `[AI SmartTalk](https://aismarttalk.tech)` |\n\n```text\nCodeblöcke bleiben im PDF gut lesbar.\n```",
    },
    "it": {
        "converter": {
            "Convert Markdown to PDF Online": "Converti Markdown in PDF online",
            "A simple Markdown to PDF converter with a direct download.": "Un convertitore semplice da Markdown a PDF con download diretto.",
            "Turn Markdown into a downloadable PDF in your browser. Keep headings, tables, links and code readable with md-to-pdf by AI SmartTalk.": "Trasforma Markdown in un PDF scaricabile direttamente dal browser. Titoli, tabelle, link e codice restano leggibili con md-to-pdf di AI SmartTalk.",
            "Convert Markdown to PDF in a few clicks": "Converti Markdown in PDF in pochi clic",
            "Paste your Markdown, create a PDF and download it. Headings, tables, links and code blocks are rendered into a document you can share.": "Incolla il tuo Markdown, crea un PDF e scaricalo. Titoli, tabelle, link e blocchi di codice vengono trasformati in un documento da condividere.",
            "ONLINE TOOL": "STRUMENTO ONLINE",
            "PDF CONVERSION · AI SMARTTALK": "CONVERSIONE PDF · AI SMARTTALK",
            "Your Markdown document": "Il tuo documento Markdown",
            "No account": "Senza account",
            "Markdown content": "Contenuto Markdown",
            "0 / 100,000 characters": "0 / 100.000 caratteri",
            "Create my PDF": "Crea il mio PDF",
            "Download the PDF": "Scarica il PDF",
            "Your document is processed for conversion. This tool returns the PDF directly, disables the shared cache and removes temporary files after the response.": "Il documento viene elaborato per la conversione. Lo strumento restituisce direttamente il PDF, disattiva la cache condivisa e rimuove i file temporanei al termine della risposta.",
            "How to convert Markdown to PDF": "Come convertire Markdown in PDF",
            "Paste or write your document in the editor.": "Incolla o scrivi il documento nell’editor.",
            "Select": "Seleziona",
            "and wait for rendering.": "e attendi la creazione del PDF.",
            "Download the generated PDF.": "Scarica il PDF generato.",
            "The converter supports common Markdown headings, lists, links, tables, quotes and code blocks. For branded layouts, JSON data and automated workflows, see the ": "Il convertitore supporta titoli, elenchi, link, tabelle, citazioni e blocchi di codice Markdown. Per layout personalizzati, dati JSON e flussi automatizzati, consulta l’",
            "PDF API for developers": "API PDF per sviluppatori",
            "Need to automate?": "Vuoi automatizzare il processo?",
            "Use the md-to-pdf API to create branded documents from your applications and workflows.": "Usa l’API md-to-pdf per creare documenti personalizzati dalle tue applicazioni e dai tuoi flussi di lavoro.",
            "Explore the API and examples →": "Scopri l’API e gli esempi →",
            "Frequently asked questions": "Domande frequenti",
            "Do I need an account?": "Serve un account?",
            "No account is needed for this quick conversion. API access requires a key so integrations can be protected and usage attributed.": "Non serve un account per questa conversione rapida. L’accesso API richiede una chiave per proteggere le integrazioni e attribuire l’utilizzo.",
            "Which Markdown elements are supported?": "Quali elementi Markdown sono supportati?",
            "Common headings, paragraphs, lists, links, quotes, tables and code blocks are converted to PDF.": "Titoli, paragrafi, elenchi, link, citazioni, tabelle e blocchi di codice comuni vengono convertiti in PDF.",
            "Is the PDF stored after conversion?": "Il PDF viene salvato dopo la conversione?",
            "No. This page returns the PDF directly and skips shared disk storage. External resources referenced by your document may be fetched by the renderer.": "No. Questa pagina restituisce direttamente il PDF senza salvarlo nello spazio condiviso. Il motore di conversione potrebbe recuperare risorse esterne collegate nel documento.",
            "More from md-to-pdf": "Altre risorse di md-to-pdf",
            "Read PDF generation guides": "Leggi le guide sulla generazione PDF",
            "Integrate the PDF API": "Integra l’API PDF",
            "Open the application": "Apri l’applicazione",
            "Online Markdown to PDF converter with direct download.": "Convertitore online da Markdown a PDF con download diretto.",
        },
        "api": {
            "PDF Generation API for Developers": "API di generazione PDF per sviluppatori",
            "Create PDFs from Markdown, HTML and JSON templates with the md-to-pdf API by AI SmartTalk. Explore curl examples, brand themes and secure API key access.": "Crea PDF da Markdown, HTML e modelli JSON con l’API md-to-pdf di AI SmartTalk. Scopri esempi curl, temi grafici e accesso sicuro tramite chiave API.",
            "Turn Markdown, HTML and structured data into production PDFs from your app.": "Genera PDF pronti per la produzione dalla tua app a partire da Markdown, HTML e dati strutturati.",
            "PDF generation API for Markdown, HTML and JSON data.": "API di generazione PDF per Markdown, HTML e dati JSON.",
            "FOR PRODUCT AND ENGINEERING TEAMS": "PER I TEAM DI PRODOTTO E SVILUPPO",
            "Generate PDFs from your app with an API": "Genera PDF dalla tua app con un’API",
            "Send Markdown, HTML or structured data. Get back a PDF ready to deliver, with brand themes, previews and document quality tools.": "Invia Markdown, HTML o dati strutturati e ricevi un PDF pronto da consegnare, con temi personalizzati, anteprime e strumenti per la qualità dei documenti.",
            "Request an API key →": "Richiedi una chiave API →",
            "Browse the reference": "Consulta la documentazione",
            "Markdown to PDF": "Da Markdown a PDF",
            "Send a report, proposal or meeting notes in Markdown, with CSS and page options.": "Invia report, proposte o note di riunione in Markdown, con opzioni CSS e di pagina.",
            "Data to PDF": "Dati in PDF",
            "Fill a Tera template with JSON data to create personalized documents repeatedly.": "Compila un modello Tera con dati JSON per creare documenti personalizzati in modo ripetibile.",
            "Inspect the output": "Controlla il risultato",
            "Preview pages, compare versions and audit layouts before delivery.": "Visualizza le pagine in anteprima, confronta le versioni e verifica l’impaginazione prima della consegna.",
            "API capabilities": "Funzionalità dell’API",
            "Example: convert a Markdown document": "Esempio: convertire un documento Markdown",
            "Every API request uses a customer-specific key in the ": "Ogni richiesta API usa una chiave specifica del cliente nell’intestazione ",
            "header. Administrators can create and revoke each key independently.": ". Gli amministratori possono creare e revocare ogni chiave separatamente.",
            "Keep the key on your server or in your secrets manager. Never ship it in browser JavaScript.": "Conserva la chiave sul server o in un gestore di segreti. Non inserirla mai nel JavaScript del browser.",
            "Try the public converter": "Prova il convertitore pubblico",
            "Preview how a Markdown document renders before connecting an integration.": "Verifica il risultato della conversione di un documento Markdown prima di collegare un’integrazione.",
            "Open the Markdown to PDF tool →": "Apri lo strumento Markdown in PDF →",
            "One engine, multiple workflows": "Un motore, tanti flussi di lavoro",
            "The service brings together conversion, template rendering, preview, merge, watermark, password protection, redaction and layout analysis. API operations require a key so access stays protected and usage can be attributed to each integration.": "Il servizio riunisce conversione, rendering di modelli, anteprima, unione, filigrana, protezione con password, oscuramento dei dati e analisi dell’impaginazione. Le operazioni API richiedono una chiave per proteggere l’accesso e attribuire l’utilizzo a ogni integrazione.",
            "Browse every endpoint in the interactive reference": "Consulta tutti gli endpoint nella documentazione interattiva",
            " or ": " oppure ",
            "request an API key": "richiedi una chiave API",
            "Guides and examples": "Guide ed esempi",
            "Read integration guides": "Leggi le guide all’integrazione",
            "Convert an example online": "Converti un esempio online",
        },
        "sample": "# Il mio documento\n\nScrivi in **Markdown**, aggiungi un elenco o una tabella e crea il PDF.\n\n| Elemento | Esempio |\n| --- | --- |\n| Titolo | `# Report` |\n| Elenco | `- Primo passaggio` |\n| Link | `[AI SmartTalk](https://aismarttalk.tech)` |\n\n```text\nI blocchi di codice restano leggibili nel PDF.\n```",
    },
    "pt": {
        "converter": {
            "Convert Markdown to PDF Online": "Converter Markdown para PDF online",
            "A simple Markdown to PDF converter with a direct download.": "Um conversor simples de Markdown para PDF com descarga direta.",
            "Turn Markdown into a downloadable PDF in your browser. Keep headings, tables, links and code readable with md-to-pdf by AI SmartTalk.": "Transforme Markdown num PDF para descarregar diretamente no navegador. Títulos, tabelas, links e código ficam legíveis com o md-to-pdf da AI SmartTalk.",
            "Convert Markdown to PDF in a few clicks": "Converta Markdown em PDF com poucos cliques",
            "Paste your Markdown, create a PDF and download it. Headings, tables, links and code blocks are rendered into a document you can share.": "Cole o seu Markdown, crie um PDF e descarregue-o. Títulos, tabelas, links e blocos de código são convertidos num documento pronto para partilhar.",
            "ONLINE TOOL": "FERRAMENTA ONLINE",
            "PDF CONVERSION · AI SMARTTALK": "CONVERSÃO DE PDF · AI SMARTTALK",
            "Your Markdown document": "O seu documento Markdown",
            "No account": "Sem conta",
            "Markdown content": "Conteúdo Markdown",
            "0 / 100,000 characters": "0 / 100.000 caracteres",
            "Create my PDF": "Criar o meu PDF",
            "Download the PDF": "Descarregar o PDF",
            "Your document is processed for conversion. This tool returns the PDF directly, disables the shared cache and removes temporary files after the response.": "O seu documento é processado para conversão. A ferramenta devolve o PDF diretamente, desativa a cache partilhada e remove os ficheiros temporários após a resposta.",
            "How to convert Markdown to PDF": "Como converter Markdown em PDF",
            "Paste or write your document in the editor.": "Cole ou escreva o seu documento no editor.",
            "Select": "Selecione",
            "and wait for rendering.": "e aguarde pela criação do PDF.",
            "Download the generated PDF.": "Descarregue o PDF gerado.",
            "The converter supports common Markdown headings, lists, links, tables, quotes and code blocks. For branded layouts, JSON data and automated workflows, see the ": "O conversor suporta títulos, listas, links, tabelas, citações e blocos de código comuns em Markdown. Para layouts de marca, dados JSON e fluxos automatizados, consulte a ",
            "PDF API for developers": "API de PDF para programadores",
            "Need to automate?": "Precisa de automatizar?",
            "Use the md-to-pdf API to create branded documents from your applications and workflows.": "Use a API md-to-pdf para criar documentos com a sua marca a partir das suas aplicações e fluxos de trabalho.",
            "Explore the API and examples →": "Explore a API e os exemplos →",
            "Frequently asked questions": "Perguntas frequentes",
            "Do I need an account?": "Preciso de uma conta?",
            "No account is needed for this quick conversion. API access requires a key so integrations can be protected and usage attributed.": "Não precisa de uma conta para esta conversão rápida. O acesso à API requer uma chave para proteger as integrações e atribuir a utilização.",
            "Which Markdown elements are supported?": "Que elementos Markdown são suportados?",
            "Common headings, paragraphs, lists, links, quotes, tables and code blocks are converted to PDF.": "Títulos, parágrafos, listas, links, citações, tabelas e blocos de código comuns são convertidos em PDF.",
            "Is the PDF stored after conversion?": "O PDF fica guardado após a conversão?",
            "No. This page returns the PDF directly and skips shared disk storage. External resources referenced by your document may be fetched by the renderer.": "Não. Esta página devolve o PDF diretamente e não o guarda no armazenamento partilhado. O motor de conversão poderá aceder a recursos externos referenciados no documento.",
            "More from md-to-pdf": "Mais recursos do md-to-pdf",
            "Read PDF generation guides": "Leia os guias de geração de PDF",
            "Integrate the PDF API": "Integre a API de PDF",
            "Open the application": "Abrir a aplicação",
            "Online Markdown to PDF converter with direct download.": "Conversor online de Markdown para PDF com descarga direta.",
        },
        "api": {
            "PDF Generation API for Developers": "API de geração de PDF para programadores",
            "Create PDFs from Markdown, HTML and JSON templates with the md-to-pdf API by AI SmartTalk. Explore curl examples, brand themes and secure API key access.": "Crie PDFs a partir de Markdown, HTML e modelos JSON com a API md-to-pdf da AI SmartTalk. Consulte exemplos curl, temas de marca e acesso seguro com chave de API.",
            "Turn Markdown, HTML and structured data into production PDFs from your app.": "Gere PDFs prontos para produção na sua aplicação a partir de Markdown, HTML e dados estruturados.",
            "PDF generation API for Markdown, HTML and JSON data.": "API de geração de PDF para Markdown, HTML e dados JSON.",
            "FOR PRODUCT AND ENGINEERING TEAMS": "PARA EQUIPAS DE PRODUTO E ENGENHARIA",
            "Generate PDFs from your app with an API": "Gere PDFs na sua aplicação com uma API",
            "Send Markdown, HTML or structured data. Get back a PDF ready to deliver, with brand themes, previews and document quality tools.": "Envie Markdown, HTML ou dados estruturados e receba um PDF pronto a entregar, com temas de marca, pré-visualizações e ferramentas para verificar a qualidade do documento.",
            "Request an API key →": "Pedir uma chave de API →",
            "Browse the reference": "Consultar a referência",
            "Markdown to PDF": "Markdown para PDF",
            "Send a report, proposal or meeting notes in Markdown, with CSS and page options.": "Envie relatórios, propostas ou atas em Markdown, com opções de CSS e de página.",
            "Data to PDF": "Dados para PDF",
            "Fill a Tera template with JSON data to create personalized documents repeatedly.": "Preencha um modelo Tera com dados JSON para gerar documentos personalizados de forma recorrente.",
            "Inspect the output": "Verifique o resultado",
            "Preview pages, compare versions and audit layouts before delivery.": "Pré-visualize páginas, compare versões e verifique o layout antes da entrega.",
            "API capabilities": "Funcionalidades da API",
            "Example: convert a Markdown document": "Exemplo: converter um documento Markdown",
            "Every API request uses a customer-specific key in the ": "Cada pedido à API usa uma chave específica do cliente no cabeçalho ",
            "header. Administrators can create and revoke each key independently.": ". Os administradores podem criar e revogar cada chave de forma independente.",
            "Keep the key on your server or in your secrets manager. Never ship it in browser JavaScript.": "Guarde a chave no seu servidor ou num gestor de segredos. Nunca a inclua no JavaScript do navegador.",
            "Try the public converter": "Experimente o conversor público",
            "Preview how a Markdown document renders before connecting an integration.": "Veja como um documento Markdown é convertido antes de ligar uma integração.",
            "Open the Markdown to PDF tool →": "Abrir a ferramenta Markdown para PDF →",
            "One engine, multiple workflows": "Um motor para vários fluxos de trabalho",
            "The service brings together conversion, template rendering, preview, merge, watermark, password protection, redaction and layout analysis. API operations require a key so access stays protected and usage can be attributed to each integration.": "O serviço reúne conversão, geração por modelos, pré-visualização, junção, marcas de água, proteção por palavra-passe, ocultação de dados e análise de layout. As operações da API requerem uma chave para proteger o acesso e atribuir a utilização a cada integração.",
            "Browse every endpoint in the interactive reference": "Consulte todos os endpoints na referência interativa",
            " or ": " ou ",
            "request an API key": "pedir uma chave de API",
            "Guides and examples": "Guias e exemplos",
            "Read integration guides": "Leia os guias de integração",
            "Convert an example online": "Converta um exemplo online",
        },
        "sample": "# O meu documento\n\nEscreva em **Markdown**, adicione uma lista ou tabela e crie o seu PDF.\n\n| Elemento | Exemplo |\n| --- | --- |\n| Título | `# Relatório` |\n| Lista | `- Primeiro passo` |\n| Link | `[AI SmartTalk](https://aismarttalk.tech)` |\n\n```text\nOs blocos de código continuam legíveis no PDF.\n```",
    },
}


def navigation(language: str, page_kind: str) -> str:
    ui = UI[language]
    converter, api = landing_paths(language)
    if page_kind == "converter":
        links = (
            f'<a href="{converter}">{ui["convert"]}</a><a href="/{language}/blog">{ui["guides"]}</a>'
            f'<a href="{api}">{ui["api"]}</a><a href="/#/acces">{ui["developer_access"]}</a>'
        )
    else:
        links = (
            f'<a href="{converter}">{ui["convert_online"]}</a><a href="/{language}/blog">{ui["guides"]}</a>'
            f'<a href="/#/api">{ui["api_reference"]}</a><a href="/#/acces">{ui["request_access"]}</a>'
        )
    language_links = " ".join(
        f'<a lang="{code}" href="{landing_paths(code)[0 if page_kind == "converter" else 1]}">{BY_CODE[code]["native_name"]}</a>'
        for code in sorted(BY_CODE)
    )
    return (
        f'<nav aria-label="{ui["main"]}">{links}</nav>'
        f'<nav class="language-nav" aria-label="{ui["languages"]}">{language_links}</nav>'
    )


def language_navigation(language: str, page_kind: str) -> str:
    route_index = 0 if page_kind == "converter" else 1
    links = " ".join(
        f'<a lang="{code}" href="{landing_paths(code)[route_index]}">{BY_CODE[code]["native_name"]}</a>'
        for code in sorted(BY_CODE)
    )
    label = {
        "fr": "Langues", "en": "Languages", **{code: values["languages"] for code, values in UI.items()}
    }[language]
    return f'<nav class="language-nav" aria-label="{label}">{links}</nav>'


def update_hreflang(page: str, kind: str) -> str:
    page = re.sub(
        r'  <link rel="alternate" hreflang="[^"]+" href="[^"]+">\n?',
        "",
        page,
    )
    route_index = 0 if kind == "converter" else 1
    alternates = [
        f'  <link rel="alternate" hreflang="{code}" href="{SITE_URL}{landing_paths(code)[route_index]}">'
        for code in sorted(BY_CODE)
    ]
    default_route = landing_paths(DEFAULT_LANGUAGE)[route_index]
    alternates.append(f'  <link rel="alternate" hreflang="x-default" href="{SITE_URL}{default_route}">')
    return re.sub(
        r'  <link rel="canonical" href="[^"]+">',
        lambda match: match.group(0) + "\n" + "\n".join(alternates),
        page,
        count=1,
    )


def landing_paths(language: str) -> tuple[str, str]:
    paths = BY_CODE[language].get("landing_paths")
    if not isinstance(paths, dict) or not isinstance(paths.get("converter"), str) or not isinstance(paths.get("api"), str):
        raise SystemExit(f"Routes produit absentes du catalogue pour {language}.")
    return paths["converter"], paths["api"]


def translate_page(source: str, language: str, kind: str) -> str:
    copy = COPY[language]
    translations = copy[kind]
    page = source.replace('<html lang="en">', f'<html lang="{language}">', 1)
    page = re.sub(r'<nav class="language-nav"[^>]*>.*?</nav>', "", page, flags=re.S)

    converter, api = landing_paths(language)
    for old, new in (
        (f"{SITE_URL}/en/markdown-to-pdf", f"{SITE_URL}{converter}"),
        (f"{SITE_URL}/en/pdf-generation-api", f"{SITE_URL}{api}"),
        ("/en/markdown-to-pdf", converter),
        ("/en/pdf-generation-api", api),
        ("/en/blog", f"/{language}/blog"),
    ):
        page = page.replace(old, new)

    page = update_hreflang(page, kind)
    page = re.sub(r'<nav aria-label="Main navigation">.*?</nav>', navigation(language, kind), page, count=1, flags=re.S)

    footer_converter = UI[language]["footer_converter"]
    footer = (
        f'<footer class="site-footer"><span>md-to-pdf by AI SmartTalk</span><nav>'
        f'<a href="{converter}">{footer_converter}</a>'
        f'<a href="/{language}/blog">{UI[language]["guides"]}</a>'
        f'<a href="{api}">{UI[language]["api"]}</a>'
        f'<a href="https://aismarttalk.tech">AI SmartTalk</a></nav></footer>'
    )
    page = re.sub(
        r'<footer class="site-footer"><span>md-to-pdf by AI SmartTalk</span><nav>.*?</nav></footer>',
        footer,
        page,
        count=1,
        flags=re.S,
    )

    for original, translated in sorted(translations.items(), key=lambda pair: len(pair[0]), reverse=True):
        if original not in page:
            raise SystemExit(f"Texte source absent ({language}/{kind}) : {original}")
        page = page.replace(original, translated)

    if kind == "converter":
        page, count = re.subn(
            r'(<textarea id="markdown"[^>]*>).*?(</textarea>)',
            lambda match: match.group(1) + copy["sample"] + match.group(2),
            page,
            count=1,
            flags=re.S,
        )
        if count != 1:
            raise SystemExit(f"Exemple du convertisseur absent ({language}).")
    return page


def build() -> None:
    for language in COPY:
        if language not in BY_CODE:
            raise SystemExit(f"Langue sans entrée dans locales.json : {language}")
        converter, api = landing_paths(language)
        for kind, source_name, path in (
            ("converter", "markdown-to-pdf.html", converter),
            ("api", "pdf-generation-api.html", api),
        ):
            source = (ROOT / "static" / "seo" / "en" / source_name).read_text(encoding="utf-8")
            page = translate_page(source, language, kind)
            destination = ROOT / "static" / "seo" / language / f"{Path(path).name}.html"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(page, encoding="utf-8")
            destination.chmod(0o644)
    core_files = {
        ("fr", "converter"): ROOT / "static" / "seo" / "markdown-to-pdf.html",
        ("fr", "api"): ROOT / "static" / "seo" / "markdown-to-pdf-api.html",
        ("en", "converter"): ROOT / "static" / "seo" / "en" / "markdown-to-pdf.html",
        ("en", "api"): ROOT / "static" / "seo" / "en" / "pdf-generation-api.html",
    }
    for (language, kind), path in core_files.items():
        page = path.read_text(encoding="utf-8")
        page = re.sub(r'<a lang="(?:fr|en)" href="/(?:fr|en)/[^\"]+">[^<]+</a>', "", page)
        page = re.sub(r'<nav class="language-nav"[^>]*>.*?</nav>', "", page, flags=re.S)
        page = update_hreflang(page, kind)
        page = page.replace("</header>", language_navigation(language, kind) + "</header>", 1)
        path.write_text(page, encoding="utf-8")
    configured = set(BY_CODE) - {"fr", "en"}
    if set(COPY) != configured:
        raise SystemExit(f"Traductions de pages manquantes : {', '.join(sorted(configured - set(COPY)))}")


if __name__ == "__main__":
    try:
        build()
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"Impossible de générer les pages produit : {exc}") from exc
