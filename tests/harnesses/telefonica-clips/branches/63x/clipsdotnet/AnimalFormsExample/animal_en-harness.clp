; Harness for telefonica-clips/branches/63x/clipsdotnet/AnimalFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-fd007170b10bedf02cf787e79dafaf1cefa0afed7964c95c14e705d19c527a0e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-fd007170b10bedf02cf787e79dafaf1cefa0afed7964c95c14e705d19c527a0e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fd007170b10bedf02cf787e79dafaf1cefa0afed7964c95c14e705d19c527a0e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fd007170b10bedf02cf787e79dafaf1cefa0afed7964c95c14e705d19c527a0e|COMPLETE" crlf))
