# MySQL 8.0 (Ubuntu 24.04 noble の `mysql-server` package) を install し、
# isucon user / nrb2026 database を準備する。bind は 127.0.0.1 のみ。
#
# webapp は DATABASE_URL=mysql://isucon:isucon@127.0.0.1:3306/nrb2026 で接続する
# (mitamae/cookbooks/webapp/default.rb の systemd unit を参照)。
#
# 認証 plugin は ISUCON 慣例どおり mysql_native_password にする (sqlx 等のクライアントが
# caching_sha2_password を喋らない/設定が面倒なケースを避ける)。

package "mysql-server"

# my.cnf override: bind-address は default の 127.0.0.1 のままで触らない。
# default-authentication-plugin だけ揃える。
file "/etc/mysql/mysql.conf.d/99-nrb2026.cnf" do
  owner "root"
  group "root"
  mode "0644"
  content <<~CNF
    [mysqld]
    default_authentication_plugin = mysql_native_password
  CNF
end

service "mysql" do
  action [:enable, :start]
end

execute "restart mysql to pick up 99-nrb2026.cnf" do
  command "systemctl restart mysql"
  not_if "mysql -uroot -e \"SHOW VARIABLES LIKE 'default_authentication_plugin'\" 2>/dev/null | grep -q mysql_native_password"
end

# isucon user (mysql_native_password) を作る。既に正しく作られていれば skip。
execute "create isucon mysql user" do
  command <<~SQL.gsub("\n", " ").strip
    mysql -uroot -e "
      CREATE USER 'isucon'@'localhost' IDENTIFIED WITH mysql_native_password BY 'isucon';
      CREATE USER 'isucon'@'127.0.0.1' IDENTIFIED WITH mysql_native_password BY 'isucon';
    "
  SQL
  not_if "mysql -uroot -e \"SELECT user FROM mysql.user WHERE user='isucon'\" 2>/dev/null | grep -q isucon"
end

execute "create nrb2026 database" do
  command "mysql -uroot -e \"CREATE DATABASE IF NOT EXISTS nrb2026 CHARACTER SET utf8mb4 COLLATE utf8mb4_bin\""
end

execute "grant nrb2026 privileges to isucon" do
  command <<~SQL.gsub("\n", " ").strip
    mysql -uroot -e "
      GRANT ALL PRIVILEGES ON nrb2026.* TO 'isucon'@'localhost';
      GRANT ALL PRIVILEGES ON nrb2026.* TO 'isucon'@'127.0.0.1';
      FLUSH PRIVILEGES;
    "
  SQL
end
