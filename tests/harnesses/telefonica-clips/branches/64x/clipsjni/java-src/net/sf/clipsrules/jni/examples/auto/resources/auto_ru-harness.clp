; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-6940a5824611c0c91b29d8c584f2e25a7819a83731cfcd6e689386dd352134a3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-6940a5824611c0c91b29d8c584f2e25a7819a83731cfcd6e689386dd352134a3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6940a5824611c0c91b29d8c584f2e25a7819a83731cfcd6e689386dd352134a3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6940a5824611c0c91b29d8c584f2e25a7819a83731cfcd6e689386dd352134a3|COMPLETE" crlf))
