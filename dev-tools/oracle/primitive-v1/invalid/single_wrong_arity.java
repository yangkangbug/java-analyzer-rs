// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:single_wrong_arity*/m() {} static void use() { /*Q:single_wrong_arity*/m(1); } }
