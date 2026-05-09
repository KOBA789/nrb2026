# TODO: boot 時に /etc/hosts を動的に書く systemd unit (v1 の isunarabe-env-ipaddr.service パターン)
# - cloud-init の manage_etc_hosts と競合しないために、cookbook 適用時には /etc/hosts を
#   静的に書かず、boot 時に systemd unit が書く形にする
# - nrb2026 は HTTP/80 + IP 直アクセス (docs/authoring/platform.md 付録 A) なので TLS SNI/
#   FQDN ルーティングのための解決上書きは不要。webapp ↔ 隣接サービスの private IP alias が
#   必要になった段階で実装する
