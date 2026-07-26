; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AnimalWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a274ef31c5eeeadbc8c877b25988999cbd4c8e637eb16fc617d36710c1feb220-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a274ef31c5eeeadbc8c877b25988999cbd4c8e637eb16fc617d36710c1feb220|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a274ef31c5eeeadbc8c877b25988999cbd4c8e637eb16fc617d36710c1feb220|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a274ef31c5eeeadbc8c877b25988999cbd4c8e637eb16fc617d36710c1feb220|COMPLETE" crlf))
