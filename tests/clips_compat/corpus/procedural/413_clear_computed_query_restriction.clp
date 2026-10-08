(deftemplate item (slot x))
(deffacts seed (item (x 1)) (item (x 2)))
(defrule clear-from-eval
   =>
   (printout t "found="
      (eval "(progn (clear) (find-all-facts ((?f (sym-cat item))) TRUE))") crlf)
   (printout t (length$ (find-all-facts ((?f item)) TRUE)) " items" crlf))
