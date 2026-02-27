(deftemplate foo (multislot bar) (multislot yak))
(deffacts init
   (foo (bar) (yak)))
(deffunction callit (?c)
    (loop-for-count (?i ?c)
       (do-for-fact ((?f foo)) TRUE
          (bind ?b1 ?f:bar)
          (bind ?b2 ?f:yak)
          (assert (foo (bar ?b1 ?i) (yak ?b2 (- ?c ?i))))
          (retract ?f))))
(defrule doit
    =>
    (callit 2000))
