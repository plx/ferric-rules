(deffunction f () 7)
(defrule clear-before-call
   =>
   (printout t "r=" (progn (clear) (f)) crlf)
   (printout t "r2=" (eval "(progn (clear) (f))") crlf)
   (printout t "r3=" (eval "(progn (clear) (+ 1 2))") crlf))
