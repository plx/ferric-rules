; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2ddf4048db164881d2f86a2c63d66ef5bca333f229fb2a3fca99982b49fc782c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2ddf4048db164881d2f86a2c63d66ef5bca333f229fb2a3fca99982b49fc782c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2ddf4048db164881d2f86a2c63d66ef5bca333f229fb2a3fca99982b49fc782c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2ddf4048db164881d2f86a2c63d66ef5bca333f229fb2a3fca99982b49fc782c|COMPLETE" crlf))
