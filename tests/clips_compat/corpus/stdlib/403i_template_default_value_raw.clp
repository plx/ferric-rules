;; A queried dynamic default is the evaluated expression: a void result stays
;; void and a multifield stays whole, even for a single-field slot. Multislot
;; defaults splice their elements and omit void ones.
(deffunction two () (create$ 1 2))
(deffunction none () (printout t ""))
(deftemplate item
  (slot x (default-dynamic (two)))
  (slot z (default-dynamic (none)))
  (multislot m (default-dynamic (none) (two) 3)))
(defrule probe =>
  (printout t "[" (deftemplate-slot-default-value item z) "]" crlf)
  (printout t (deftemplate-slot-default-value item x) " "
    (length$ (deftemplate-slot-default-value item x)) crlf)
  (printout t (deftemplate-slot-default-value item m) crlf)
  (printout t "after" crlf))
