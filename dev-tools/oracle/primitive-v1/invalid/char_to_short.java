// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:char_to_short_candidate_one*/m(byte a) {} static void /*D:char_to_short_candidate_two*/m(short a) {} static void use(char c) { /*Q:char_to_short*/m(c); } }
