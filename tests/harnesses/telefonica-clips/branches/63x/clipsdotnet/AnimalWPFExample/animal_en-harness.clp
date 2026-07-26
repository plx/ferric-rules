; Harness for telefonica-clips/branches/63x/clipsdotnet/AnimalWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7aee8ecabd11d3a0d3d11082320d025c837d077b1c6184311e2d5ff2201f986b-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7aee8ecabd11d3a0d3d11082320d025c837d077b1c6184311e2d5ff2201f986b|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7aee8ecabd11d3a0d3d11082320d025c837d077b1c6184311e2d5ff2201f986b|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7aee8ecabd11d3a0d3d11082320d025c837d077b1c6184311e2d5ff2201f986b|COMPLETE" crlf))
