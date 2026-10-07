(deffunction mark (?x) (printout t ?x ";") ?x)
(deftemplate sample
 (slot a (default-dynamic (mark DA)))
 (slot b (default-dynamic (mark DB)))
 (slot c (default-dynamic (mark DC))))
(defrule run =>
 (printout t "one:" (assert (sample (b (mark EB)))) crlf)
 (printout t "two:" (assert (sample (c (mark EC)) (a (mark EA)))) crlf))
