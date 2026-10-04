(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(deffacts other (item 4 5))
(defrule query =>
(do-for-all-facts ((?f p item q)) TRUE
 (printout t (fact-relation ?f) ":" (if (eq (fact-relation ?f) item) then ?f:implied else ?f:x) crlf))
)
