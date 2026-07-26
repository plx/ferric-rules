; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-e8e813f0880789fdfb8a519d86062020d506fee4b24d20644b68341737e5cd31-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e813f0880789fdfb8a519d86062020d506fee4b24d20644b68341737e5cd31|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e813f0880789fdfb8a519d86062020d506fee4b24d20644b68341737e5cd31|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e8e813f0880789fdfb8a519d86062020d506fee4b24d20644b68341737e5cd31|COMPLETE" crlf))
