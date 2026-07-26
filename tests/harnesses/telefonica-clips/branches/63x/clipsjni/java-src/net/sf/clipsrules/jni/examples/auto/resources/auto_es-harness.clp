; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9cc2fdcdafef5b533204f0725426501d7305b52e92c226d04e94bd18618683c6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9cc2fdcdafef5b533204f0725426501d7305b52e92c226d04e94bd18618683c6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9cc2fdcdafef5b533204f0725426501d7305b52e92c226d04e94bd18618683c6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9cc2fdcdafef5b533204f0725426501d7305b52e92c226d04e94bd18618683c6|COMPLETE" crlf))
