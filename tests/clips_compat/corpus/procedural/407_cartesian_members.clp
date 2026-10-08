(deftemplate p (slot x))
(deftemplate q (slot x))
(deftemplate empty)
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule query =>
(do-for-all-facts ((?a p) (?b p q)) TRUE (printout t ?a:x ":" ?b:x ":" (eq ?a ?b) crlf))
)
