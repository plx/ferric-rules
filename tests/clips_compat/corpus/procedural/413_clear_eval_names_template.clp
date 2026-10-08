(deftemplate item (slot x))
(deffacts seed (item (x 1)))
(defrule clear-from-eval
   =>
   (eval "(progn (assert (item (x 2))) (clear))")
   (printout t (length$ (find-all-facts ((?f item)) TRUE)) " items" crlf))
