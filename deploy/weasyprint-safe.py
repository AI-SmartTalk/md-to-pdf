#!/usr/bin/python3
# -*- coding: utf-8 -*-
# urlguard-wrapper — marqueur lu par le Dockerfile pour rendre l'installation
# idempotente : sans lui, une reconstruction renommerait l'enveloppe en
# weasyprint-real et elle s'appellerait elle-même indéfiniment.
"""
Enveloppe de sécurité autour de WeasyPrint, installée comme /usr/local/bin/weasyprint.

WeasyPrint résout TOUTE URL référencée par le document : <img src>, url() en CSS,
@import, références SVG, @font-face. Sur un service exposé, cela donne un SSRF vers
le réseau interne (y compris en aveugle, par mesure du temps de réponse) et une
lecture de fichiers locaux via file://. Le CLI d'origine n'offre aucune prise sur
ces accès : seule l'API Python accepte un `url_fetcher`. D'où ce script.

Il DOIT s'appeler `weasyprint` : pandoc choisit la forme de ses arguments d'après le
nom de base passé à --pdf-engine, et refuse tout nom qu'il ne connaît pas. Il accepte
donc la même ligne de commande que le binaire d'origine (déplacé en
`weasyprint-real`) et ignore proprement les options qu'il ne connaît pas.

Un refus n'interrompt JAMAIS le rendu : la ressource est remplacée par un corps vide
et le refus part sur stderr (et sur PDF_URLGUARD_LOG s'il est défini). Un document au
visuel imparfait vaut mieux qu'un 500 sur un service de production.

Variables d'environnement :
  PDF_ALLOWED_URL_HOSTS   liste d'hôtes autorisés, séparés par des virgules. Vide =
                          tout hôte PUBLIC est accepté (les adresses privées restent
                          refusées) ; c'est le comportement historique du service.
  PDF_URL_STRICT_HOSTS    à vrai, une liste vide veut dire « aucun hôte autorisé ».
  PDF_ALLOW_LOCAL_ASSETS  autorise file:// sous le répertoire de travail (défaut vrai).
  PDF_URLGUARD_ALLOW_FILES  chemins supplémentaires autorisés en file://, séparés par
                          « : » (l'appelant y met les fichiers qu'il a lui-même créés).
  PDF_URLGUARD_TIMEOUT_SECS / PDF_URLGUARD_MAX_BYTES / PDF_URLGUARD_MAX_REDIRECTS
  PDF_URLGUARD_LOG        fichier où recopier les refus (sinon stderr seul).
  PDF_URL_GUARD=off       trappe de secours : redonne la main au binaire d'origine.
"""

import http.client
import ipaddress
import mimetypes
import os
import socket
import ssl
import sys
import tempfile
from urllib.parse import urljoin, urlsplit, unquote
from urllib.request import url2pathname, urlopen

REAL_BINARY = os.environ.get("WEASYPRINT_REAL", "/usr/local/bin/weasyprint-real")

DEFAULT_TIMEOUT = 10.0
DEFAULT_MAX_BYTES = 10 * 1024 * 1024
DEFAULT_MAX_REDIRECTS = 3

# Noms qui ne peuvent désigner qu'une ressource interne, quelle que soit la
# résolution DNS du moment.
DENIED_HOSTNAMES = frozenset(
    (
        "localhost",
        "instance-data",
        "metadata",
        "metadata.google.internal",
        "metadata.goog",
    )
)
DENIED_HOST_SUFFIXES = (
    ".localhost",
    ".local",
    ".internal",
    ".intranet",
    ".home.arpa",
)


def env_flag(name, default):
    raw = os.environ.get(name)
    if raw is None or not raw.strip():
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


def env_number(name, default):
    raw = os.environ.get(name)
    if raw is None or not raw.strip():
        return default
    try:
        value = type(default)(raw.strip())
    except (TypeError, ValueError):
        return default
    return value if value > 0 else default


def env_hosts(name):
    raw = os.environ.get(name) or ""
    return frozenset(h.strip().lower().rstrip(".") for h in raw.split(",") if h.strip())


def warn(message):
    line = "[urlguard] %s\n" % message
    sys.stderr.write(line)
    sys.stderr.flush()
    # Le service parent ne remonte la sortie d'erreur qu'en cas d'échec du
    # processus ; un refus, lui, réussit. D'où ce second canal pour l'exploitant.
    path = os.environ.get("PDF_URLGUARD_LOG")
    if path:
        try:
            with open(path, "a", encoding="utf-8") as handle:
                handle.write(line)
        except OSError:
            pass


class Denied(Exception):
    """Refus de politique : rattrapé sur place, jamais propagé au rendu."""


def is_public_address(ip):
    """Une entrée DNS autorisée peut pointer vers 169.254.169.254 ou 10.0.0.x."""
    if ip.version == 6:
        mapped = ip.ipv4_mapped or getattr(ip, "sixtofour", None)
        if mapped is not None:
            ip = mapped
    return not (
        ip.is_private
        or ip.is_loopback
        or ip.is_link_local
        or ip.is_multicast
        or ip.is_reserved
        or ip.is_unspecified
    )


class PinnedHTTPConnection(http.client.HTTPConnection):
    """Connexion vers une IP déjà validée, en gardant le nom d'hôte pour l'en-tête Host.

    Sans épinglage, le nom serait résolu une seconde fois par la pile réseau et une
    zone DNS hostile pourrait renvoyer une adresse publique à la vérification puis
    une adresse interne à la connexion.
    """

    def __init__(self, host, ip, port, timeout):
        super().__init__(host, port, timeout=timeout)
        self._ip = ip

    def connect(self):
        self.sock = socket.create_connection((self._ip, self.port), self.timeout)


class PinnedHTTPSConnection(http.client.HTTPSConnection):
    def __init__(self, host, ip, port, timeout, context):
        super().__init__(host, port, timeout=timeout, context=context)
        self._ip = ip

    def connect(self):
        sock = socket.create_connection((self._ip, self.port), self.timeout)
        # server_hostname = le nom, pas l'IP : le certificat reste vérifié.
        self.sock = self._context.wrap_socket(sock, server_hostname=self.host)


class GuardedFetcher(object):
    """`url_fetcher` appliquant la politique, à passer à weasyprint.HTML / weasyprint.CSS."""

    def __init__(self, extra_files=()):
        self.allow_local = env_flag("PDF_ALLOW_LOCAL_ASSETS", True)
        self.allowed_hosts = env_hosts("PDF_ALLOWED_URL_HOSTS")
        self.strict_hosts = env_flag("PDF_URL_STRICT_HOSTS", False)
        self.timeout = env_number("PDF_URLGUARD_TIMEOUT_SECS", DEFAULT_TIMEOUT)
        self.max_bytes = env_number("PDF_URLGUARD_MAX_BYTES", DEFAULT_MAX_BYTES)
        self.max_redirects = env_number("PDF_URLGUARD_MAX_REDIRECTS", DEFAULT_MAX_REDIRECTS)
        self.follow_redirects = True
        self.workdir = os.path.realpath(os.getcwd())
        self.tempdir = os.path.realpath(tempfile.gettempdir())
        # Les PDF déjà produits appartiennent à d'autres clients, et le cache est
        # adressé par contenu : ni l'un ni l'autre n'a à entrer dans un rendu.
        self.denied_dirs = [
            os.path.join(self.workdir, "public", "pdf"),
            os.path.join(self.workdir, "public", "cache"),
            self.tempdir,
        ]
        self.allowed_files = set(os.path.realpath(p) for p in extra_files)
        for path in (os.environ.get("PDF_URLGUARD_ALLOW_FILES") or "").split(":"):
            if path.strip():
                self.allowed_files.add(os.path.realpath(path.strip()))

    # -- politique ---------------------------------------------------------

    def check_host(self, host):
        host = host.lower().rstrip(".")
        if not host:
            raise Denied("hôte vide")
        if host in DENIED_HOSTNAMES or host.endswith(DENIED_HOST_SUFFIXES):
            raise Denied("hôte interne %r" % host)
        if self.allowed_hosts:
            for allowed in self.allowed_hosts:
                if host == allowed or host.endswith("." + allowed):
                    return
            raise Denied("hôte %r hors de PDF_ALLOWED_URL_HOSTS" % host)
        if self.strict_hosts:
            raise Denied("aucun hôte autorisé (PDF_ALLOWED_URL_HOSTS est vide)")

    def resolve(self, host, port):
        try:
            infos = socket.getaddrinfo(host, port, 0, socket.SOCK_STREAM)
        except socket.gaierror as error:
            raise Denied("résolution impossible de %r (%s)" % (host, error))
        addresses = []
        for info in infos:
            try:
                ip = ipaddress.ip_address(info[4][0])
            except ValueError:
                continue
            # Un seul enregistrement interne suffit à condamner le nom : sinon il
            # reste exploitable en rejouant la requête jusqu'à tomber dessus.
            if not is_public_address(ip):
                raise Denied("%r résout vers l'adresse non publique %s" % (host, ip))
            addresses.append(ip)
        if not addresses:
            raise Denied("aucune adresse utilisable pour %r" % host)
        return addresses[0]

    def check_local_path(self, path):
        real = os.path.realpath(path)
        if real in self.allowed_files:
            return real
        if self.is_app_stylesheet(real):
            return real
        if not self.allow_local:
            raise Denied("file:// désactivé (PDF_ALLOW_LOCAL_ASSETS)")
        if self.workdir == os.sep:
            # Un service lancé depuis la racine rendrait la règle « sous le
            # répertoire de travail » sans aucun effet.
            raise Denied("répertoire de travail à la racine, file:// refusé")
        if real != self.workdir and not real.startswith(self.workdir + os.sep):
            raise Denied("%s est hors du répertoire de travail" % real)
        for denied in self.denied_dirs:
            if real == denied or real.startswith(denied + os.sep):
                raise Denied("%s est dans un répertoire interdit" % real)
        return real

    def is_app_stylesheet(self, real):
        """La feuille de style que pandoc référence via --css vit dans le répertoire
        temporaire : la refuser rendrait tous les documents sans style. Le nom est
        tiré au sort par le service et une feuille de style ne peut rien exfiltrer."""
        head, name = os.path.split(real)
        return (
            head == self.tempdir
            and name.startswith(".tmp")
            and name.endswith(".css")
        )

    # -- récupération ------------------------------------------------------

    def __call__(self, url):
        return self.fetch(url)

    def fetch(self, url, headers=None):
        scheme = urlsplit(url).scheme.lower()
        try:
            if scheme == "data":
                return self.fetch_data(url)
            if scheme == "file":
                return self.fetch_file(url)
            if scheme in ("http", "https"):
                return self.fetch_http(url)
            raise Denied("schéma %r interdit" % scheme)
        except Denied as denial:
            warn("refus de %s : %s" % (url, denial))
            return response_for(url, b"")

    def fetch_data(self, url):
        with urlopen(url) as handle:  # noqa: S310 - data: ne touche pas le réseau
            body = handle.read(self.max_bytes + 1)
            content_type = handle.headers.get("Content-Type")
        if len(body) > self.max_bytes:
            raise Denied("ressource data: au-delà de %d octets" % self.max_bytes)
        return response_for(url, body, content_type)

    def fetch_file(self, url):
        split = urlsplit(url)
        if split.netloc and split.netloc.lower() not in ("", "localhost"):
            raise Denied("hôte %r interdit sur file://" % split.netloc)
        path = url2pathname(split.path)
        real = self.check_local_path(path)
        try:
            size = os.path.getsize(real)
        except OSError as error:
            raise Denied("lecture impossible (%s)" % error)
        if size > self.max_bytes:
            raise Denied("fichier de %d octets, au-delà de la limite" % size)
        try:
            with open(real, "rb") as handle:
                body = handle.read(self.max_bytes)
        except OSError as error:
            raise Denied("lecture impossible (%s)" % error)
        return response_for(url, body, mimetypes.guess_type(real)[0])

    def fetch_http(self, url, depth=0):
        split = urlsplit(url)
        host = split.hostname or ""
        self.check_host(host)
        port = split.port or (443 if split.scheme.lower() == "https" else 80)
        ip = self.resolve(host, port)

        target = split.path or "/"
        if split.query:
            target += "?" + split.query

        if split.scheme.lower() == "https":
            connection = PinnedHTTPSConnection(
                host, str(ip), port, self.timeout, ssl.create_default_context()
            )
        else:
            connection = PinnedHTTPConnection(host, str(ip), port, self.timeout)

        try:
            connection.request(
                "GET", target, headers={"User-Agent": "md-to-pdf", "Accept": "*/*"}
            )
            resp = connection.getresponse()
            status = resp.status
            location = resp.getheader("Location")
            content_type = resp.getheader("Content-Type")
            length = resp.getheader("Content-Length")
            if 300 <= status < 400 and location:
                # Chaque saut repasse la politique : une redirection est le moyen le
                # plus simple de faire pointer un hôte autorisé vers 169.254.169.254.
                if not self.follow_redirects:
                    raise Denied("redirection refusée vers %s" % location)
                if depth >= self.max_redirects:
                    raise Denied("trop de redirections")
                return self.fetch_http(urljoin(url, location), depth + 1)
            if status >= 400:
                raise Denied("réponse HTTP %d" % status)
            if length is not None:
                try:
                    if int(length) > self.max_bytes:
                        raise Denied("réponse de %s octets, au-delà de la limite" % length)
                except ValueError:
                    pass
            body = resp.read(self.max_bytes + 1)
        except Denied:
            raise
        except (OSError, http.client.HTTPException, ssl.SSLError) as error:
            raise Denied("échec de la requête (%s)" % error)
        finally:
            connection.close()

        if len(body) > self.max_bytes:
            raise Denied("réponse au-delà de %d octets" % self.max_bytes)
        return response_for(url, body, content_type)


def response_for(url, body, content_type=None):
    """Réponse au format attendu par la version de WeasyPrint installée."""
    if not content_type:
        # Un type deviné depuis l'extension évite qu'un refus se transforme en
        # « unsupported stylesheet type » là où une feuille vide suffit.
        guessed = mimetypes.guess_type(urlsplit(url).path)[0]
        content_type = guessed or "application/octet-stream"
    try:
        from weasyprint.urls import URLFetcherResponse
    except ImportError:
        URLFetcherResponse = None
    if URLFetcherResponse is not None:
        return URLFetcherResponse(url, body, {"Content-Type": content_type})
    mime, _, parameters = content_type.partition(";")
    encoding = None
    for parameter in parameters.split(";"):
        name, _, value = parameter.partition("=")
        if name.strip().lower() == "charset":
            encoding = value.strip().strip('"') or None
    resource = {"string": body, "mime_type": mime.strip(), "redirected_url": url}
    if encoding:
        resource["encoding"] = encoding
    return resource


# --- ligne de commande ------------------------------------------------------

# Options du CLI d'origine qui consomment une valeur. Celles dont la valeur n'est
# pas reprise sont acceptées puis ignorées : mieux vaut un PDF sans l'option qu'un
# échec de conversion.
VALUE_OPTIONS = {
    "-s": "stylesheet",
    "--stylesheet": "stylesheet",
    "-a": "attachment",
    "--attachment": "attachment",
    "-e": "encoding",
    "--encoding": "encoding",
    "-m": "media_type",
    "--media-type": "media_type",
    "-u": "base_url",
    "--base-url": "base_url",
    "-t": "timeout",
    "--timeout": "timeout",
    "-D": "dpi",
    "--dpi": "dpi",
    "-j": "jpeg_quality",
    "--jpeg-quality": "jpeg_quality",
    "-c": "ignored",
    "--cache-folder": "ignored",
    "--pdf-variant": "pdf_variant",
    "--pdf-version": "pdf_version",
    "--pdf-identifier": "pdf_identifier",
    "--xmp-metadata": "ignored",
    "--output-intent": "ignored",
    "--attachment-relationship": "ignored",
    "--allowed-protocols": "ignored",
}

FLAG_OPTIONS = {
    "-p": "presentational_hints",
    "--presentational-hints": "presentational_hints",
    "--optimize-images": "optimize_images",
    "--full-fonts": "full_fonts",
    "--hinting": "hinting",
    "--uncompressed-pdf": "uncompressed_pdf",
    "--pdf-forms": "pdf_forms",
    "--pdf-tags": "pdf_tags",
    "--custom-metadata": "custom_metadata",
    "--no-http-redirects": "no_redirects",
    "--fail-on-http-errors": "fail_on_errors",
    "-v": "verbose",
    "--verbose": "verbose",
    "-d": "debug",
    "--debug": "debug",
    "-q": "quiet",
    "--quiet": "quiet",
}

# Options qui n'impriment que du texte : le binaire d'origine reste la référence.
PASSTHROUGH_OPTIONS = ("-h", "--help", "--version", "-i", "--info")

# Options passées telles quelles à write_pdf, quand elles ont été demandées.
RENDER_FLAGS = (
    "presentational_hints",
    "optimize_images",
    "full_fonts",
    "hinting",
    "uncompressed_pdf",
    "pdf_forms",
    "pdf_tags",
    "custom_metadata",
)
RENDER_VALUES = ("dpi", "jpeg_quality", "pdf_variant", "pdf_version", "pdf_identifier")


def parse_args(argv):
    positional = []
    options = {}
    index = 0
    while index < len(argv):
        argument = argv[index]
        if argument == "--":
            positional.extend(argv[index + 1 :])
            break
        if argument.startswith("-") and argument != "-":
            name, equal, value = argument.partition("=")
            if name in VALUE_OPTIONS:
                if not equal:
                    index += 1
                    value = argv[index] if index < len(argv) else ""
                options.setdefault(VALUE_OPTIONS[name], []).append(value)
            elif name in FLAG_OPTIONS and not equal:
                options.setdefault(FLAG_OPTIONS[name], []).append(True)
            elif (
                len(name) > 2
                and not name.startswith("--")
                and all("-" + letter in FLAG_OPTIONS for letter in name[1:])
            ):
                for letter in name[1:]:
                    options.setdefault(FLAG_OPTIONS["-" + letter], []).append(True)
            else:
                warn("option ignorée : %s" % argument)
        else:
            positional.append(argument)
        index += 1
    return positional, options


def first(options, key, default=None):
    values = options.get(key)
    return values[0] if values else default


def run_real(argv):
    os.execv(REAL_BINARY, [REAL_BINARY] + list(argv))


def configure_logging(options):
    import logging

    level = logging.WARNING
    if options.get("quiet"):
        level = logging.ERROR
    elif options.get("debug"):
        level = logging.DEBUG
    elif options.get("verbose"):
        level = logging.INFO
    handler = logging.StreamHandler(sys.stderr)
    logger = logging.getLogger("weasyprint")
    logger.setLevel(level)
    logger.addHandler(handler)


def main(argv):
    if any(argument in PASSTHROUGH_OPTIONS for argument in argv):
        run_real(argv)
    if not env_flag("PDF_URL_GUARD", True):
        warn("garde désactivée par PDF_URL_GUARD, appel du binaire d'origine")
        run_real(argv)

    positional, options = parse_args(argv)
    if len(positional) < 2:
        sys.stderr.write("usage: weasyprint [options] input output\n")
        return 2

    source, target = positional[0], positional[1]
    if len(positional) > 2:
        warn("arguments positionnels ignorés : %r" % (positional[2:],))

    try:
        import weasyprint
    except ImportError as error:
        warn("weasyprint introuvable (%s), appel du binaire d'origine" % error)
        run_real(argv)
        return 1

    configure_logging(options)

    stylesheets = options.get("stylesheet", [])
    attachments = options.get("attachment", [])
    # Les fichiers nommés sur la ligne de commande viennent de l'appelant, pas du
    # document : ils restent lisibles où qu'ils soient.
    known_files = [source] + stylesheets + attachments
    fetcher = GuardedFetcher(extra_files=[p for p in known_files if p != "-"])
    if options.get("no_redirects"):
        fetcher.follow_redirects = False
    if options.get("fail_on_errors"):
        # Lu par weasyprint.urls.fetch pour décider si l'échec est fatal.
        fetcher._fail_on_errors = True
    timeout = first(options, "timeout")
    if timeout:
        try:
            fetcher.timeout = float(timeout)
        except ValueError:
            warn("--timeout %r illisible, valeur par défaut conservée" % timeout)

    base_url = first(options, "base_url")
    encoding = first(options, "encoding")
    media_type = first(options, "media_type")

    document_arguments = {"url_fetcher": fetcher}
    if base_url:
        document_arguments["base_url"] = base_url
    if encoding:
        document_arguments["encoding"] = encoding
    if media_type:
        document_arguments["media_type"] = media_type

    if source == "-":
        html = weasyprint.HTML(file_obj=sys.stdin.buffer, **document_arguments)
    else:
        html = weasyprint.HTML(filename=source, **document_arguments)

    styles = [
        weasyprint.CSS(filename=path, url_fetcher=fetcher) for path in stylesheets
    ]

    render_arguments = {"stylesheets": styles}
    if attachments:
        render_arguments["attachments"] = attachments
    for flag in RENDER_FLAGS:
        if options.get(flag):
            render_arguments[flag] = True
    for name in RENDER_VALUES:
        value = first(options, name)
        if value is None:
            continue
        if name in ("dpi", "jpeg_quality"):
            # Le CLI d'origine convertit ces deux-là en entier ; les passer en
            # chaîne ferait échouer le rendu au lieu de l'améliorer.
            try:
                value = int(value)
            except ValueError:
                warn("--%s %r illisible, option ignorée" % (name, value))
                continue
        render_arguments[name] = value

    destination = sys.stdout.buffer if target == "-" else target
    try:
        html.write_pdf(destination, **render_arguments)
    except TypeError as error:
        # Une option de rendu inconnue de la version installée ne doit pas coûter
        # le document : on retente avec le strict nécessaire.
        warn("options de rendu refusées (%s), nouvelle tentative sans elles" % error)
        html.write_pdf(destination, stylesheets=styles)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
