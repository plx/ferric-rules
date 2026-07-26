; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-eb269f37fd3ddc0e0bea502bfcc5ad085671c7096a1d2122437f83039f756fdf-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb269f37fd3ddc0e0bea502bfcc5ad085671c7096a1d2122437f83039f756fdf|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb269f37fd3ddc0e0bea502bfcc5ad085671c7096a1d2122437f83039f756fdf|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb269f37fd3ddc0e0bea502bfcc5ad085671c7096a1d2122437f83039f756fdf|COMPLETE" crlf))
