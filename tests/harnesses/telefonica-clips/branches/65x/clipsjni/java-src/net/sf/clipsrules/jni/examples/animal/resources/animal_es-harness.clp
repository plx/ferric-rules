; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-db2d7c61228bb1932888eca4f72d4099b3126e1c65bb5968227a58a3ff53ba37-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-db2d7c61228bb1932888eca4f72d4099b3126e1c65bb5968227a58a3ff53ba37|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-db2d7c61228bb1932888eca4f72d4099b3126e1c65bb5968227a58a3ff53ba37|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-db2d7c61228bb1932888eca4f72d4099b3126e1c65bb5968227a58a3ff53ba37|COMPLETE" crlf))
