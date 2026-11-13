// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
import com.sun.source.tree.*;
import com.sun.source.util.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import javax.lang.model.element.*;
import javax.lang.model.type.*;
import javax.lang.model.util.*;
import javax.tools.*;

/** Optional compiler-only declaration oracle. Does not generate or execute fixture programs. */
public final class BinaryDeclarationOracle {
    private static String descriptor(TypeMirror input, Types types, Elements elements) {
        TypeMirror type = types.erasure(input);
        return switch (type.getKind()) {
            case BOOLEAN -> "Z"; case BYTE -> "B"; case CHAR -> "C"; case SHORT -> "S";
            case INT -> "I"; case LONG -> "J"; case FLOAT -> "F"; case DOUBLE -> "D"; case VOID -> "V";
            case ARRAY -> "[" + descriptor(((ArrayType) type).getComponentType(), types, elements);
            case DECLARED -> "L" + elements.getBinaryName((TypeElement) ((DeclaredType) type).asElement()).toString().replace('.', '/') + ";";
            default -> throw new IllegalArgumentException("Unsupported descriptor type " + type);
        };
    }
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]).toAbsolutePath().normalize();
        List<String> requests = Files.readAllLines(Path.of(args[1]));
        List<Path> files = new ArrayList<>();
        for (String request : requests) {
            String[] cells = request.split("\t");
            if (cells[0].equals("F")) files.add(root.resolve(cells[1]));
        }
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
        try (StandardJavaFileManager fm = compiler.getStandardFileManager(diagnostics, Locale.ROOT, StandardCharsets.UTF_8)) {
            JavacTask task = (JavacTask) compiler.getTask(null, fm, diagnostics,
                List.of("--release", "21", "-proc:none", "-implicit:none", "-encoding", "UTF-8",
                        "-classpath", args[2], "-sourcepath", args[3]), null, fm.getJavaFileObjectsFromPaths(files));
            List<CompilationUnitTree> units = new ArrayList<>();
            task.parse().forEach(units::add);
            task.analyze();
            for (Diagnostic<?> d : diagnostics.getDiagnostics()) if (d.getKind() == Diagnostic.Kind.ERROR)
                System.err.println(d.getCode() + ": " + d.getMessage(Locale.ROOT));
            if (diagnostics.getDiagnostics().stream().anyMatch(d -> d.getKind() == Diagnostic.Kind.ERROR)) System.exit(2);
            Trees trees = Trees.instance(task);
            SourcePositions positions = trees.getSourcePositions();
            Set<String> emitted = new HashSet<>();
            for (CompilationUnitTree unit : units) {
                String path = root.relativize(Path.of(unit.getSourceFile().toUri())).toString().replace('\\', '/');
                String source = unit.getSourceFile().getCharContent(true).toString();
                new TreePathScanner<Void, Void>() {
                    @Override public Void visitMemberSelect(MemberSelectTree tree, Void p) {
                        long end = positions.getEndPosition(unit, tree);
                        if (end >= 0) {
                            String name = tree.getIdentifier().toString();
                            int at = source.substring(0, Math.toIntExact(end) - name.length()).getBytes(StandardCharsets.UTF_8).length;
                            for (String request : requests) {
                                String[] cells = request.split("\t");
                                if (!cells[0].equals("Q") || !cells[2].equals(path) || Integer.parseInt(cells[3]) != at) continue;
                                if (!emitted.add(cells[1])) throw new IllegalStateException("Duplicate query");
                                Element element = trees.getElement(getCurrentPath());
                                if (!(element instanceof ExecutableElement method) || method.getKind() != ElementKind.METHOD)
                                    throw new IllegalStateException("No method declaration");
                                if (trees.getPath(method) != null) throw new IllegalStateException("Expected binary-only declaration");
                                String owner = task.getElements().getBinaryName((TypeElement) method.getEnclosingElement()).toString().replace('.', '/');
                                StringBuilder desc = new StringBuilder("(");
                                for (VariableElement parameter : method.getParameters())
                                    desc.append(descriptor(parameter.asType(), task.getTypes(), task.getElements()));
                                desc.append(")").append(descriptor(method.getReturnType(), task.getTypes(), task.getElements()));
                                System.out.printf("%s\t%s\t%s\t%s%n", cells[1], owner, name, desc);
                            }
                        }
                        return super.visitMemberSelect(tree, p);
                    }
                }.scan(unit, null);
            }
            if (emitted.size() != requests.stream().filter(s -> s.startsWith("Q\t")).count())
                throw new IllegalStateException("Missing query");
        }
    }
}

