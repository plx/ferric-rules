; Harness for telefonica-clips/clipscgi/animal/animalcgi.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
