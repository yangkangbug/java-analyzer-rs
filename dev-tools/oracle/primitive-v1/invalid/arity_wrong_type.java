// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:arity_wrong_type_candidate_one*/m(long a) {} static void /*D:arity_wrong_type_candidate_two*/m(long a,long b) {} static void use(boolean b) { /*Q:arity_wrong_type*/m(b); } }
