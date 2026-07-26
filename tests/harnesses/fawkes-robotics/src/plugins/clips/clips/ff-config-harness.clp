; Harness for fawkes-robotics/src/plugins/clips/clips/ff-config.clp
; Detected constructs: deftemplate: confval
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-eb0b89885945c059b45207ff038fa95fd62fcc07a0764b5ef8a0c99dae65eb41-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb0b89885945c059b45207ff038fa95fd62fcc07a0764b5ef8a0c99dae65eb41|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb0b89885945c059b45207ff038fa95fd62fcc07a0764b5ef8a0c99dae65eb41|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb0b89885945c059b45207ff038fa95fd62fcc07a0764b5ef8a0c99dae65eb41|COMPLETE" crlf))
