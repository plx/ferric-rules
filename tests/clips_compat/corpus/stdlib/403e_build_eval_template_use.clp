;; An eval expression keeps the templates and relations it names in use while
;; it runs, although the calling rule does not name them.
(deftemplate p (slot x))
(defrule r =>
  (printout t (eval "(progn (build \"(deftemplate p (slot y))\") (assert (p (x 41))))") crlf)
  (printout t (eval "(assert (trigger (build \"(deftemplate p (slot z))\")) (p (x 42)))") crlf)
  (printout t (eval "(assert (trigger (build \"(deftemplate q (slot a))\")) (q 1))") crlf)
  (printout t (deftemplate-slot-names p) " " (deftemplate-slot-names q) " "
    (fact-relation 4) " " (fact-slot-value 4 implied) crlf)
  (eval "(do-for-all-facts ((?f p)) TRUE (printout t p \" \" ?f:x crlf))"))
