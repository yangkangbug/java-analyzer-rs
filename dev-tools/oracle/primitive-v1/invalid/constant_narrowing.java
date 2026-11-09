// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:constant_narrowing_candidate_one*/m(byte a) {} static void /*D:constant_narrowing_candidate_two*/m(short a) {} static void use() { /*Q:constant_narrowing*/m(1); } }
