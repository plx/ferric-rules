(deftemplate item (slot v))
(deffacts seed (item (v 1)) (item (v 2)) (item (v 3)))
(defrule run =>
  (printout t (find-fact ((?a item)) (> ?a:v 1)) crlf)
  (printout t (find-all-facts ((?a item) (?b item)) (< ?a:v ?b:v)) crlf)
  (do-for-fact ((?a item)) (= ?a:v 1) (printout t "first:" ?a crlf))
  (do-for-all-facts ((?a item)) (> ?a:v 1) (printout t "all:" ?a crlf))
  (delayed-do-for-all-facts ((?a item)) (= ?a:v 3) (printout t "delayed:" ?a crlf)))
