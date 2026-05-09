packer {
  required_plugins {
    amazon = {
      version = ">= 1.3.0"
      source  = "github.com/hashicorp/amazon"
    }
  }
}

variable "region" {
  type        = string
  default     = "ap-northeast-1"
  description = "AMI を焼く region。問題の deploy 想定リージョンと一致させる。"
}

variable "source_ami" {
  type        = string
  default     = "ami-0c20109cc7514960f"
  description = "Ubuntu 24.04 noble release-20260321 (ap-northeast-1)。docs/authoring/build-pipeline.md § 4 と一致。"
}

variable "version" {
  type        = string
  default     = "0.0.1"
  description = "AMI 名 / タグに焼き込む version 文字列。"
}

variable "instance_type" {
  type    = string
  default = "c5.large"
}

source "amazon-ebs" "nrb2026" {
  region        = var.region
  source_ami    = var.source_ami
  ssh_username  = "ubuntu"
  instance_type = var.instance_type
  ami_name      = "nrb2026-${var.version}-{{timestamp}}"
  #ami_groups    = ["all"]

  launch_block_device_mappings {
    device_name           = "/dev/sda1"
    volume_size           = 16
    volume_type           = "gp3"
    delete_on_termination = true
  }

  tags = {
    Name             = "nrb2026"
    IsunarabeProblem = "nrb2026"
    IsunarabeVersion = var.version
  }
}

build {
  sources = ["source.amazon-ebs.nrb2026"]

  provisioner "file" {
    source      = "../build/payload.tar.gz"
    destination = "/tmp/payload.tar.gz"
  }

  provisioner "shell" {
    inline_shebang = "/bin/bash -e"
    inline = [
      "set -e -o pipefail",
      "cd /tmp",
      "tar xzf payload.tar.gz",
      "sudo /tmp/payload/mitamae/mitamae local /tmp/payload/mitamae/roles/default.rb",
    ]
  }

  # AMI snapshot を絞るための sysprep。ISUCON 10 final base.libsonnet:207-225 に倣う:
  #   * /tmp/payload* (~3 GB) を捨てる
  #   * journal / apt の package archives (/var/cache/apt/archives) を削る
  #     (apt-get clean は archives のみ。/var/lib/apt/lists は競技者の apt-get update を
  #      速くするため残す)
  #   * fstrim で trim 済み領域を snapshot から外す (-a で全 mount。将来 /var を別 mount
  #     にしても漏れない)
  provisioner "shell" {
    inline_shebang = "/bin/bash -e"
    inline = [
      "set -e -o pipefail",
      "sudo rm -rf /tmp/payload /tmp/payload.tar.gz",
      "sudo journalctl --rotate || :",
      "sudo journalctl --vacuum-time=1s || :",
      "sudo apt-get clean",
      "sudo /sbin/fstrim -av || :",
    ]
  }
}
