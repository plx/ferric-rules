(deffacts startup
    (candidate old)
    (candidate new))

(defrule choose-old
    (candidate old)
    (not (winner ?))
    =>
    (assert (winner old))
    (assert (extra)))

(defrule choose-new
    (candidate new)
    (not (winner ?))
    =>
    (assert (winner new)))

(defrule observe-extra
    (extra)
    =>)
