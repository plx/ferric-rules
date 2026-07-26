; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d5163975aab00820ff7d1f82aee1ea38a423c7ccb9c5890e4df30bee67fb15f7-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5163975aab00820ff7d1f82aee1ea38a423c7ccb9c5890e4df30bee67fb15f7|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5163975aab00820ff7d1f82aee1ea38a423c7ccb9c5890e4df30bee67fb15f7|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5163975aab00820ff7d1f82aee1ea38a423c7ccb9c5890e4df30bee67fb15f7|COMPLETE" crlf))
