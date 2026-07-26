; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AutoWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-dd8f2e6e82bdc014c56879b8227b54df5a4cffd27d35674c0fa4d6c78e6f4cbe-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-dd8f2e6e82bdc014c56879b8227b54df5a4cffd27d35674c0fa4d6c78e6f4cbe|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-dd8f2e6e82bdc014c56879b8227b54df5a4cffd27d35674c0fa4d6c78e6f4cbe|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-dd8f2e6e82bdc014c56879b8227b54df5a4cffd27d35674c0fa4d6c78e6f4cbe|COMPLETE" crlf))
