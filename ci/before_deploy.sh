# This script takes care of building your crate and packaging it for release

set -ex

main() {

    export GIT_VERSION=$(git describe --tags --dirty=-modified)

    local src=$(pwd) \
          stage=

    case $TRAVIS_OS_NAME in
        linux)
            stage=$(mktemp -d)
            ;;
        osx)
            stage=$(mktemp -d -t tmp)
            ;;
    esac

    test -f Cargo.lock || cargo generate-lockfile

    # TODO Update this to build the artifacts that matter to you
    cross rustc --bin exclave --target $TARGET --release -- -C lto

    # TODO Update this to package the right artifacts
    if [ -e target/$TARGET/release/exclave.exe ]
    then
        ext=.exe
    else
        ext=
    fi
    cp target/$TARGET/release/exclave$ext $stage/

    cd $stage
    tar czf $src/$CRATE_NAME-$TRAVIS_TAG-$TARGET.tar.gz *
    cd $src

    rm -rf $stage
}

main
