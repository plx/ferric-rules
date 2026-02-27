; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AutoFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
