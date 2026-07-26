; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-c11c9c2bce9ec0ab6f70d2f830650bc2be14ebdece1bd4e5f1a6d37d8be29586-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-c11c9c2bce9ec0ab6f70d2f830650bc2be14ebdece1bd4e5f1a6d37d8be29586|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c11c9c2bce9ec0ab6f70d2f830650bc2be14ebdece1bd4e5f1a6d37d8be29586|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c11c9c2bce9ec0ab6f70d2f830650bc2be14ebdece1bd4e5f1a6d37d8be29586|COMPLETE" crlf))
