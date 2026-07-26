; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b8c2ede46ae173e081651ff80ffea3ccef286ea4ce171a7c1752add1e814171a-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b8c2ede46ae173e081651ff80ffea3ccef286ea4ce171a7c1752add1e814171a|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b8c2ede46ae173e081651ff80ffea3ccef286ea4ce171a7c1752add1e814171a|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b8c2ede46ae173e081651ff80ffea3ccef286ea4ce171a7c1752add1e814171a|COMPLETE" crlf))
