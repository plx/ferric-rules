; Harness for fawkes-robotics/src/plugins/clips-executive/clips/cx-identity.clp
; Detected constructs: deffunction: cx-identity-set/1, cx-identity/0
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
