; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-f89c1cae982c0ade94fbcda80f8438e2ed631e4787c7a5c59afe9416661313f6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-f89c1cae982c0ade94fbcda80f8438e2ed631e4787c7a5c59afe9416661313f6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f89c1cae982c0ade94fbcda80f8438e2ed631e4787c7a5c59afe9416661313f6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f89c1cae982c0ade94fbcda80f8438e2ed631e4787c7a5c59afe9416661313f6|COMPLETE" crlf))
