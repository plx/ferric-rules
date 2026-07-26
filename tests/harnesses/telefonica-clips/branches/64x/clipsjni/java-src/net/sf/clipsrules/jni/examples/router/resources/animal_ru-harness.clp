; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-e8e84bbf0b962e6e953d93bc07fdd7b7e78cbb14daa2465bd61a05f3124b1e44-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e84bbf0b962e6e953d93bc07fdd7b7e78cbb14daa2465bd61a05f3124b1e44|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e84bbf0b962e6e953d93bc07fdd7b7e78cbb14daa2465bd61a05f3124b1e44|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e84bbf0b962e6e953d93bc07fdd7b7e78cbb14daa2465bd61a05f3124b1e44|COMPLETE" crlf))
